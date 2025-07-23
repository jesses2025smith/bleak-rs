use btleplug::{
    api::{Central, CentralEvent, Manager as _, Peripheral as _},
    platform::{Adapter, Manager, Peripheral, PeripheralId},
    Error,
};
use std::{
    collections::HashSet,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock, Weak,
    },
    time::{Duration, Instant},
};
use stream_cancel::{Trigger, Valved};
use tokio::sync::broadcast::{self, Sender};
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use uuid::Uuid;

use crate::{Device, DeviceEvent};

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Filter {
    Address(String),
    Characteristic(Uuid),
    Name(String),
    Rssi(i16),
    Service(Uuid),
}

#[derive(Default)]
pub struct ScanConfig {
    /// Index of the Bluetooth adapter to use. The first found adapter is used by default.
    adapter_index: usize,
    /// Filters objects
    filters: Vec<Filter>,
    /// Filters the found devices based on device address.
    address_filter: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
    /// Filters the found devices based on local name.
    name_filter: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
    /// Filters the found devices based on rssi.
    rssi_filter: Option<Box<dyn Fn(i16) -> bool + Send + Sync>>,
    /// Filters the found devices based on service's uuid.
    service_filter: Option<Box<dyn Fn(&Vec<Uuid>, &Uuid) -> bool + Send + Sync>>,
    /// Filters the found devices based on characteristics. Requires a connection to the device.
    characteristics_filter: Option<Box<dyn Fn(&Vec<Uuid>) -> bool + Send + Sync>>,
    /// Maximum results before the scan is stopped.
    max_results: Option<usize>,
    /// The scan is stopped when timeout duration is reached.
    timeout: Option<Duration>,
    /// Force disconnect when listen the device is connected.
    force_disconnect: bool,
}

impl ScanConfig {
    /// Index of bluetooth adapter to use
    #[inline]
    pub fn adapter_index(mut self, index: usize) -> Self {
        self.adapter_index = index;
        self
    }

    #[inline]
    pub fn with_filters(mut self, filters: &[Filter]) -> Self {
        self.filters.extend_from_slice(filters);
        self
    }

    /// Filter scanned devices based on the device address
    #[inline]
    pub fn filter_by_address(
        mut self,
        func: impl Fn(&str) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.address_filter = Some(Box::new(func));
        self
    }

    /// Filter scanned devices based on the device name
    #[inline]
    pub fn filter_by_name(mut self, func: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.name_filter = Some(Box::new(func));
        self
    }

    #[inline]
    pub fn filter_by_rssi(mut self, func: impl Fn(i16) -> bool + Send + Sync + 'static) -> Self {
        self.rssi_filter = Some(Box::new(func));
        self
    }

    #[inline]
    pub fn filter_by_service(
        mut self,
        func: impl Fn(&Vec<Uuid>, &Uuid) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.service_filter = Some(Box::new(func));
        self
    }

    /// Filter scanned devices based on available characteristics
    #[inline]
    pub fn filter_by_characteristics(
        mut self,
        func: impl Fn(&Vec<Uuid>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.characteristics_filter = Some(Box::new(func));
        self
    }

    /// Stop the scan after given number of matches
    #[inline]
    pub fn stop_after_matches(mut self, max_results: usize) -> Self {
        self.max_results = Some(max_results);
        self
    }

    /// Stop the scan after the first match
    #[inline]
    pub fn stop_after_first_match(self) -> Self {
        self.stop_after_matches(1)
    }

    /// Stop the scan after given duration
    #[inline]
    pub fn stop_after_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    #[inline]
    pub fn force_disconnect(mut self, force_disconnect: bool) -> Self {
        self.force_disconnect = force_disconnect;
        self
    }

    /// Require that the scanned devices have a name
    #[inline]
    pub fn require_name(self) -> Self {
        if self.name_filter.is_none() {
            self.filter_by_name(|src| !src.is_empty())
        } else {
            self
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Session {
    pub(crate) _manager: Manager,
    pub(crate) adapter: Adapter,
}

#[derive(Debug, Clone)]
pub struct Scanner {
    session: Weak<Session>,
    event_sender: Sender<DeviceEvent>,
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

        let stream: Pin<Box<dyn Stream<Item = DeviceEvent> + Send>> =
            Box::pin(BroadcastStream::new(receiver).filter_map(|x| match x {
                Ok(event) => {
                    log::debug!("Broadcasting device: {:?}", event);
                    Some(event)
                }
                Err(e) => {
                    log::warn!("Error: {:?} when broadcasting device event!", e);
                    None
                }
            }));

        let (trigger, stream) = Valved::new(stream);
        self.stoppers.write()?.push(trigger);

        Ok(stream)
    }

    /// Create a new stream that receives discovered ble devices.
    pub fn device_stream(
        &self,
    ) -> Result<Valved<Pin<Box<dyn Stream<Item = Device> + Send>>>, Error> {
        let receiver = self.event_sender.subscribe();

        let stream: Pin<Box<dyn Stream<Item = Device> + Send>> =
            Box::pin(BroadcastStream::new(receiver).filter_map(|x| match x {
                Ok(DeviceEvent::Discovered(device)) => {
                    log::debug!("Broadcasting device: {:?}", device.address());
                    Some(device)
                }
                Err(e) => {
                    log::warn!("Error: {:?} when broadcasting device!", e);
                    None
                }
                _ => None,
            }));

        let (trigger, stream) = Valved::new(stream);
        self.stoppers.write()?.push(trigger);

        Ok(stream)
    }
}

pub struct ScannerWorker {
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
    event_sender: Sender<DeviceEvent>,
    /// Stop the scan event.
    stopper: Arc<AtomicBool>,
}

impl ScannerWorker {
    fn new(
        config: ScanConfig,
        session: Arc<Session>,
        event_sender: Sender<DeviceEvent>,
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

    async fn scan(&mut self) -> Result<(), Error> {
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
                match self.event_sender.send(DeviceEvent::Updated(Device::new(
                    self.session.adapter.clone(),
                    peripheral,
                ))) {
                    Ok(value) => log::debug!("Sent device: {}, size: {}...", address, value),
                    Err(e) => log::debug!("Error: {:?} when Sending device: {}...", e, address),
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
                match self.event_sender.send(DeviceEvent::Connected(Device::new(
                    self.session.adapter.clone(),
                    peripheral,
                ))) {
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
                    .send(DeviceEvent::Disconnected(Device::new(
                        self.session.adapter.clone(),
                        peripheral,
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
                                    name_filter(name)
                                } else {
                                    name == v
                                }
                            })
                        }
                        Filter::Rssi(v) => {
                            passed &= property.rssi.is_some_and(|rssi| {
                                if let Some(rssi_filter) = &self.config.rssi_filter {
                                    rssi_filter(rssi)
                                } else {
                                    rssi >= *v
                                }
                            });
                        }
                        Filter::Service(v) => {
                            let services = &property.services;
                            if let Some(service_filter) = &self.config.service_filter {
                                passed &= service_filter(&services, v);
                            } else {
                                passed &= property.services.contains(v);
                            }
                        }
                        Filter::Address(v) => {
                            let addr = property.address.to_string();
                            if let Some(address_filter) = &self.config.address_filter {
                                passed &= address_filter(&addr);
                            } else {
                                passed &= addr == *v;
                            }
                        }
                        Filter::Characteristic(v) => {
                            let _ = self
                                .apply_character_filter(&peripheral, v, &mut passed)
                                .await;
                        }
                    }
                }

                if passed {
                    self.matched.insert(peripheral_id.clone());
                    self.result_count += 1;

                    if let Err(e) = self.event_sender.send(DeviceEvent::Discovered(Device::new(
                        self.session.adapter.clone(),
                        peripheral,
                    ))) {
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
            if self.connecting.lock()?.insert(peripheral.id()) {
                log::debug!("Connecting to device {}", peripheral.address());

                // Connect in another thread, so we can keep filtering other devices meanwhile.
                // let peripheral_clone = peripheral.clone();
                let connecting_map = self.connecting.clone();
                if let Err(e) = peripheral.connect().await {
                    log::warn!("Could not connect to {}: {:?}", peripheral.address(), e);

                    connecting_map.lock()?.remove(&peripheral.id());

                    return Ok(());
                };
            }
        }

        let mut characteristics = Vec::new();
        characteristics.extend(peripheral.characteristics());

        if self.config.force_disconnect {
            if let Err(e) = peripheral.disconnect().await {
                log::warn!("Error: {} when disconnect device", e);
            }
        }

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
                        characteristics_filter(&characteristics)
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Filter, ScanConfig, Scanner};
    use crate::Device;
    use btleplug::{api::BDAddr, Error};
    use std::{future::Future, time::Duration};
    use tokio_stream::StreamExt;
    use uuid::Uuid;

    async fn device_stream<T: Future<Output = ()>>(
        scanner: Scanner,
        callback: impl Fn(Device) -> T,
    ) {
        let duration = Duration::from_millis(15_000);
        if let Err(_) = tokio::time::timeout(duration, async move {
            if let Ok(mut stream) = scanner.device_stream() {
                while let Some(device) = stream.next().await {
                    callback(device).await;
                    break;
                }
            }
        })
        .await
        {
            eprintln!("timeout....");
        }
    }

    #[tokio::test]
    async fn test_filter_by_address() -> Result<(), Error> {
        pretty_env_logger::init();

        let mac_addr = [0xE3, 0x9E, 0x2A, 0x4D, 0xAA, 0x97];
        let filers = vec![Filter::Address("E3:9E:2A:4D:AA:97".into())];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        device_stream(scanner, |device| async move {
            assert_eq!(device.address(), BDAddr::from(mac_addr));
        })
        .await;

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_character() -> Result<(), Error> {
        pretty_env_logger::init();

        let filers = vec![Filter::Characteristic(Uuid::from_u128(
            0x6e400001_b5a3_f393_e0a9_e50e24dcca9e,
        ))];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        device_stream(scanner, |device| async move {
            println!("device: {:?} found", device);
        })
        .await;

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_name() -> Result<(), Error> {
        pretty_env_logger::init();

        let name = "73429485";
        let filers = vec![Filter::Name(name.into())];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        device_stream(scanner, |device| async move {
            assert_eq!(device.local_name().await, Some(name.into()));
        })
        .await;

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_rssi() -> Result<(), Error> {
        pretty_env_logger::init();

        let filers = vec![Filter::Rssi(-70)];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        device_stream(scanner, |device| async move {
            println!("device: {:?} found", device);
        })
        .await;

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_service() -> Result<(), Error> {
        pretty_env_logger::init();

        let service = Uuid::from_u128(0x6e400001_b5a3_f393_e0a9_e50e24dcca9e);
        let filers = vec![Filter::Service(service)];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        device_stream(scanner, |device| async move {
            println!("device: {:?} found", device);
        })
        .await;

        Ok(())
    }
}
