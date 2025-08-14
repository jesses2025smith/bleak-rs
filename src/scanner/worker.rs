use btleplug::{
    api::{Central, CentralEvent, Peripheral as _},
    platform::{Adapter, Manager, Peripheral, PeripheralId},
    Error,
};
use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};
use tokio::sync::broadcast::Sender;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::{
    scanner::config::{Filter, ScanConfig},
    Device, DeviceEvent,
};

#[derive(Debug, Clone)]
pub(crate) enum ScanEvent {
    DeviceEvent(DeviceEvent),
    ScanStopped,
}

#[derive(Debug, Clone)]
pub(crate) struct Session {
    pub(crate) _manager: Manager,
    pub(crate) adapter: Adapter,
}

pub(crate) struct ScannerWorker {
    /// Configurations for the scan, such as filters and stop conditions
    config: ScanConfig,
    /// Reference to the bluetooth session instance
    session: Arc<Session>,
    /// Number of matching devices found so far
    result_count: usize,
    /// Set of devices that have been filtered and will be ignored
    filtered: HashSet<PeripheralId>,
    /// Set of devices that we are currently connecting to
    connecting: Arc<Mutex<HashSet<PeripheralId>>>,
    /// Set of devices that matched the filters
    matched: HashSet<PeripheralId>,
    /// Channel for sending events to the client
    event_sender: Sender<ScanEvent>,
    /// Stop the scan event.
    stopper: Arc<AtomicBool>,
}

impl ScannerWorker {
    pub fn new(
        config: ScanConfig,
        session: Arc<Session>,
        event_sender: Sender<ScanEvent>,
        stopper: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config,
            session,
            result_count: 0,
            filtered: HashSet::new(),
            connecting: Arc::new(Mutex::new(HashSet::new())),
            matched: HashSet::new(),
            event_sender,
            stopper,
        }
    }

    pub async fn scan(&mut self) -> Result<(), Error> {
        log::info!("Starting the scan");

        self.session.adapter.start_scan(Default::default()).await?;

        while let Ok(mut stream) = self.session.adapter.events().await {
            let start_time = Instant::now();

            while let Some(event) = stream.next().await {
                match event {
                    CentralEvent::DeviceDiscovered(v) => self.on_device_discovered(v).await,
                    CentralEvent::DeviceUpdated(v) => self.on_device_updated(v).await,
                    CentralEvent::DeviceConnected(v) => self.on_device_connected(v).await?,
                    CentralEvent::DeviceDisconnected(v) => self.on_device_disconnected(v).await?,
                    _ => {}
                }

                let timeout_reached = self
                    .config
                    .timeout
                    .filter(|timeout| Instant::now().duration_since(start_time).ge(timeout))
                    .is_some();

                let max_result_reached = self
                    .config
                    .max_results
                    .filter(|max_results| self.result_count >= *max_results)
                    .is_some();

                if timeout_reached || max_result_reached || self.stopper.load(Ordering::Relaxed) {
                    log::info!("Scanner stop condition reached.");
                    let _ = self.event_sender.send(ScanEvent::ScanStopped);
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    async fn on_device_discovered(&mut self, peripheral_id: PeripheralId) {
        if let Ok(peripheral) = self.session.adapter.peripheral(&peripheral_id).await {
            log::trace!("Device discovered: {:?}", peripheral);

            self.apply_filter(peripheral_id).await;
        }
    }

    async fn on_device_updated(&mut self, peripheral_id: PeripheralId) {
        if let Ok(peripheral) = self.session.adapter.peripheral(&peripheral_id).await {
            log::trace!("Device updated: {:?}", peripheral);

            if self.matched.contains(&peripheral_id) {
                let address = peripheral.address();
                match self
                    .event_sender
                    .send(ScanEvent::DeviceEvent(DeviceEvent::Updated(Device::new(
                        self.session.adapter.clone(),
                        peripheral,
                    )))) {
                    Ok(value) => log::trace!("Sent device: {}, size: {}...", address, value),
                    Err(e) => log::warn!("Error: {:?} when Sending device: {}...", e, address),
                }
            } else {
                self.apply_filter(peripheral_id).await;
            }
        }
    }

    async fn on_device_connected(&mut self, peripheral_id: PeripheralId) -> Result<(), Error> {
        self.connecting.lock()?.remove(&peripheral_id);

        if let Ok(peripheral) = self.session.adapter.peripheral(&peripheral_id).await {
            log::trace!("Device connected: {:?}", peripheral);

            if self.matched.contains(&peripheral_id) {
                let address = peripheral.address();
                match self
                    .event_sender
                    .send(ScanEvent::DeviceEvent(DeviceEvent::Connected(Device::new(
                        self.session.adapter.clone(),
                        peripheral,
                    )))) {
                    Ok(value) => log::trace!("Sent device: {}, size: {}...", address, value),
                    Err(e) => log::warn!("Error: {:?} when Sending device: {}...", e, address),
                }
            } else {
                self.apply_filter(peripheral_id).await;
            }
        }

        Ok(())
    }

    async fn on_device_disconnected(&self, peripheral_id: PeripheralId) -> Result<(), Error> {
        if let Ok(peripheral) = self.session.adapter.peripheral(&peripheral_id).await {
            log::trace!("Device disconnected: {:?}", peripheral);

            if self.matched.contains(&peripheral_id) {
                let address = peripheral.address();
                match self
                    .event_sender
                    .send(ScanEvent::DeviceEvent(DeviceEvent::Disconnected(
                        Device::new(self.session.adapter.clone(), peripheral),
                    ))) {
                    Ok(value) => log::trace!("Sent device: {}, size: {}...", address, value),
                    Err(e) => log::warn!("Error: {:?} when Sending device: {}...", e, address),
                }
            }
        }

        self.connecting.lock()?.remove(&peripheral_id);

        Ok(())
    }

    async fn apply_filter(&mut self, peripheral_id: PeripheralId) {
        if self.filtered.contains(&peripheral_id) {
            return;
        }

        if let Ok(peripheral) = self.session.adapter.peripheral(&peripheral_id).await {
            if let Ok(Some(property)) = peripheral.properties().await {
                let mut passed = true;
                log::trace!("filtering: {:?}", property);

                for filter in self.config.filters.iter() {
                    if !passed {
                        break;
                    }
                    match filter {
                        Filter::Name(v) => {
                            passed &= property.local_name.as_ref().is_some_and(|name| {
                                if let Some(name_filter) = &self.config.name_filter {
                                    name_filter(name, v)
                                } else {
                                    name == v
                                }
                            })
                        }
                        Filter::Rssi(v) => {
                            passed &= property.rssi.is_some_and(|rssi| {
                                if let Some(rssi_filter) = &self.config.rssi_filter {
                                    rssi_filter(rssi, *v)
                                } else {
                                    rssi >= *v
                                }
                            });
                        }
                        Filter::Address(v) => {
                            let addr = property.address.to_string();
                            if let Some(address_filter) = &self.config.address_filter {
                                passed &= address_filter(&addr, v);
                            } else {
                                passed &= addr == *v;
                            }
                        }
                        Filter::Characteristic(v) => {
                            if let Err(e) = self
                                .apply_character_filter(&peripheral, v, &mut passed)
                                .await
                            {
                                log::warn!(
                                    "Error: {:?} when applying characteristic filter: {:?}",
                                    e,
                                    v
                                );
                            }
                        }
                        Filter::Service(v) => {
                            let services = &property.services;
                            if let Some(service_filter) = &self.config.service_filter {
                                passed &= service_filter(&services, v);
                            } else {
                                passed &= property.services.contains(v);
                            }
                        }
                    }
                }

                if passed {
                    self.matched.insert(peripheral_id.clone());
                    self.result_count += 1;

                    if let Err(e) =
                        self.event_sender
                            .send(ScanEvent::DeviceEvent(DeviceEvent::Discovered(
                                Device::new(self.session.adapter.clone(), peripheral),
                            )))
                    {
                        log::warn!("error: {} when sending device", e);
                    }
                }

                log::debug!(
                    "current matched: {}, current filtered: {}",
                    self.matched.len(),
                    self.filtered.len()
                );
            }

            self.filtered.insert(peripheral_id);
        }
    }

    async fn apply_character_filter(
        &self,
        peripheral: &Peripheral,
        uuid: &Uuid,
        passed: &mut bool,
    ) -> Result<(), Error> {
        if !peripheral.is_connected().await.unwrap_or(false) {
            let flag = { self.connecting.lock()?.insert(peripheral.id()) };
            if flag {
                log::debug!("Connecting to device {}", peripheral.address());

                // Connect in another thread, so we can keep filtering other devices meanwhile.
                // let peripheral_clone = peripheral.clone();
                if let Err(e) = peripheral.connect().await {
                    log::warn!("Could not connect to {}: {:?}", peripheral.address(), e);
                };
                let _ = { self.connecting.lock()?.remove(&peripheral.id()) };

                return Ok(());
            }
        }

        let mut characteristics = Vec::new();
        characteristics.extend(peripheral.characteristics());

        *passed &= if characteristics.is_empty() {
            let address = peripheral.address();
            log::debug!("Discovering characteristics for {}", address);

            match peripheral.discover_services().await {
                Ok(()) => {
                    characteristics.extend(peripheral.characteristics());
                    let characteristics = characteristics
                        .into_iter()
                        .map(|c| c.uuid)
                        .collect::<Vec<_>>();

                    if let Some(characteristics_filter) = &self.config.characteristics_filter {
                        characteristics_filter(&characteristics, uuid)
                    } else {
                        characteristics.contains(uuid)
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Error: `{:?}` when discovering characteristics for {}",
                        e,
                        address
                    );
                    false
                }
            }
        } else {
            true
        };

        if self.config.force_disconnect {
            if let Err(e) = peripheral.disconnect().await {
                log::warn!("Error: {} when disconnect device", e);
            }
        }

        Ok(())
    }
}
