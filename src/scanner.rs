pub(crate) mod config;
mod worker;

use btleplug::{api::Manager as _, platform::Manager, Error};
use std::{
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, RwLock, Weak,
    },
};
use stream_cancel::{Trigger, Valved};
use tokio::sync::broadcast::{self, Sender};
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};

use self::{
    config::ScanConfig,
    worker::{ScanEvent, ScannerWorker, Session},
};
use crate::{Device, DeviceEvent};

#[derive(Debug, Clone)]
pub struct Scanner {
    session: Weak<Session>,
    event_sender: Sender<ScanEvent>,
    stoppers: Arc<RwLock<Vec<Trigger>>>,
    scan_stopper: Arc<AtomicBool>,
}

impl Default for Scanner {
    fn default() -> Self {
        Scanner::new()
    }
}

impl Scanner {
    pub fn new() -> Self {
        let (event_sender, _) = broadcast::channel(32);
        Self {
            scan_stopper: Arc::new(AtomicBool::new(false)),
            session: Weak::new(),
            event_sender,
            stoppers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Start scanning for ble devices.
    pub async fn start(&mut self, config: ScanConfig) -> Result<(), Error> {
        if self.session.upgrade().is_some() {
            log::info!("Scanner is already started.");
            return Ok(());
        }

        let manager = Manager::new().await?;
        let mut adapters = manager.adapters().await?;

        if config.adapter_index >= adapters.len() {
            return Err(Error::DeviceNotFound);
        }

        let adapter = adapters.swap_remove(config.adapter_index);
        log::trace!("Using adapter: {:?}", adapter);

        let session = Arc::new(Session {
            _manager: manager,
            adapter,
        });
        self.session = Arc::downgrade(&session);

        let event_sender = self.event_sender.clone();

        let mut worker = ScannerWorker::new(
            config,
            session.clone(),
            event_sender,
            self.scan_stopper.clone(),
        );
        tokio::spawn(async move {
            let _ = worker.scan().await;
        });

        Ok(())
    }

    /// Stop scanning for ble devices.
    pub async fn stop(&self) -> Result<(), Error> {
        self.scan_stopper.store(true, Ordering::Relaxed);
        self.stoppers.write()?.clear();
        log::info!("Scanner is stopped.");

        Ok(())
    }

    /// Returns true if the scanner is active.
    pub fn is_active(&self) -> bool {
        self.session.upgrade().is_some()
    }

    /// Create a new stream that receives ble device events.
    pub fn device_event_stream(
        &self,
    ) -> Result<Valved<Pin<Box<dyn Stream<Item = DeviceEvent> + Send>>>, Error> {
        let receiver = self.event_sender.subscribe();

        let stream: Pin<Box<dyn Stream<Item = DeviceEvent> + Send>> = Box::pin(
            BroadcastStream::new(receiver)
                .take_while(|x| match x {
                    Ok(ScanEvent::ScanStopped) => {
                        log::info!("Received ScanStopped event, ending device event stream");
                        false
                    }
                    _ => true,
                })
                .filter_map(|x| match x {
                    Ok(ScanEvent::DeviceEvent(event)) => {
                        log::trace!("Broadcasting device: {:?}", event);
                        Some(event)
                    }
                    Err(e) => {
                        log::warn!("Error: {:?} when broadcasting device event!", e);
                        None
                    }
                    _ => None,
                }),
        );

        let (trigger, stream) = Valved::new(stream);
        self.stoppers.write()?.push(trigger);

        Ok(stream)
    }

    /// Create a new stream that receives discovered ble devices.
    pub fn device_stream(
        &self,
    ) -> Result<Valved<Pin<Box<dyn Stream<Item = Device> + Send>>>, Error> {
        let receiver = self.event_sender.subscribe();

        let stream: Pin<Box<dyn Stream<Item = Device> + Send>> = Box::pin(
            BroadcastStream::new(receiver)
                .take_while(|x| match x {
                    Ok(ScanEvent::ScanStopped) => {
                        log::info!("Received ScanStopped event, ending device stream");
                        false
                    }
                    _ => true,
                })
                .filter_map(|x| match x {
                    Ok(ScanEvent::DeviceEvent(DeviceEvent::Discovered(device))) => {
                        log::trace!("Broadcasting device: {:?}", device.address());
                        Some(device)
                    }
                    Err(e) => {
                        log::warn!("Error: {:?} when broadcasting device!", e);
                        None
                    }
                    _ => None,
                }),
        );

        let (trigger, stream) = Valved::new(stream);
        self.stoppers.write()?.push(trigger);

        Ok(stream)
    }
}
