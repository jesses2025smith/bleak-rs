use std::collections::HashSet;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use btleplug::api::{BDAddr, Central, CentralEvent, Manager as _, Peripheral as _};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use btleplug::Error;
use futures::{Stream, StreamExt};
use uuid::Uuid;

use crate::{Device, DeviceEvent};
use stream_cancel::{Trigger, Valved};
use tokio::sync::broadcast;
use tokio::sync::broadcast::Sender;
use tokio_stream::wrappers::BroadcastStream;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Filter {
    Address(BDAddr),
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
    address_filter: Option<Box<dyn Fn(BDAddr) -> bool + Send + Sync>>,
    /// Filters the found devices based on local name.
    name_filter: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
    /// Filters the found devices based on rssi.
    rssi_filter: Option<Box<dyn Fn(i16) -> bool + Send + Sync>>,
    /// Filters the found devices based on service's uuid.
    service_filter: Option<Box<dyn Fn(&Uuid) -> bool + Send + Sync>>,
    /// Filters the found devices based on characteristics. Requires a connection to the device.
    characteristics_filter: Option<Box<dyn Fn(&[Uuid]) -> bool + Send + Sync>>,
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
    pub fn filter_by_address(mut self, func: impl Fn(BDAddr) -> bool + Send + Sync + 'static) -> Self {
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
    pub fn filter_by_service(mut self, func: impl Fn(&Uuid) -> bool + Send + Sync + 'static) -> Self {
        self.service_filter = Some(Box::new(func));
        self
    }

    /// Filter scanned devices based on available characteristics
    #[inline]
    pub fn filter_by_characteristics(
        mut self,
        func: impl Fn(&[Uuid]) -> bool + Send + Sync + 'static,
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
            self.filter_by_name(|name| !name.is_empty())
        } else {
            self
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Session {
    pub(crate) _manager: Manager,
    pub(crate) adapter:  Adapter,
}

#[derive(Debug, Clone)]
pub struct Scanner {
    session:      Weak<Session>,
    event_sender: Sender<DeviceEvent>,
    stoppers:     Arc<RwLock<Vec<Trigger>>>,
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
            session:      Weak::new(),
            event_sender,
            stoppers:     Arc::new(RwLock::new(Vec::new())),
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
            self.scan_stopper.clone());
        tokio::spawn( async move {
            worker.scan().await;
        });

        Ok(())
    }

    /// Stop scanning for ble devices.
    pub async fn stop(&self) -> Result<(), Error> {
        self.scan_stopper.store(true, Ordering::Relaxed);
        self.stoppers.write().unwrap().clear();
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
    ) -> Valved<Pin<Box<dyn Stream<Item = DeviceEvent> + Send>>> {
        let receiver = self.event_sender.subscribe();

        let stream: Pin<Box<dyn Stream<Item = DeviceEvent> + Send>> =
            Box::pin(BroadcastStream::new(receiver).filter_map(|x| async move {
                match x {
                    Ok(event) => {
                        log::debug!("Broadcasting device: {:?}", event);
                        Some(event)

                    },
                    Err(e) => {
                        log::warn!("Error: {:?} when broadcasting device event!", e);
                        None
                    },
                }
            }));

        let (trigger, stream) = Valved::new(stream);
        self.stoppers.write().unwrap().push(trigger);

        stream
    }

    /// Create a new stream that receives discovered ble devices.
    pub fn device_stream(&self) -> Valved<Pin<Box<dyn Stream<Item = Device> + Send>>> {
        let receiver = self.event_sender.subscribe();

        let stream: Pin<Box<dyn Stream<Item = Device> + Send>> =
            Box::pin(BroadcastStream::new(receiver).filter_map(|x| async move {
                match x {
                    Ok(DeviceEvent::Discovered(device)) => {
                        log::debug!("Broadcasting device: {:?}", device.address());
                        Some(device)
                    },
                    Err(e) => {
                        log::warn!("Error: {:?} when broadcasting device!", e);
                        None
                    },
                    _ => {
                        log::warn!("Unknown error when broadcasting device!");
                        None
                    },
                }
            }));

        let (trigger, stream) = Valved::new(stream);
        self.stoppers.write().unwrap().push(trigger);

        stream
    }
}

pub struct ScannerWorker {
    /// Configurations for the scan, such as filters and stop conditions
    config:            ScanConfig,
    /// Reference to the bluetooth session instance
    session:           Arc<Session>,
    /// Number of matching devices found so far
    result_count:      usize,
    /// Set of devices that have been filtered and will be ignored
    filtered:          HashSet<PeripheralId>,
    /// Set of devices that we are currently connecting to
    connecting:        Arc<Mutex<HashSet<PeripheralId>>>,
    /// Set of devices that matched the filters
    matched:           HashSet<PeripheralId>,
    /// Channel for sending events to the client
    event_sender:      Sender<DeviceEvent>,
    /// Stop the scan event.
    stopper:           Arc<AtomicBool>,
}

impl ScannerWorker {

    fn new(
        config:       ScanConfig,
        session:      Arc<Session>,
        event_sender: Sender<DeviceEvent>,
        stopper:      Arc<AtomicBool>
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

    async fn scan(&mut self) {
        log::info!("Starting the scan");

        match self.session.adapter.start_scan(Default::default()).await {
            Ok(()) => {
                while let Ok(mut stream) = self.session.adapter.events().await {
                    let start_time = Instant::now();

                    while let Some(event) = stream.next().await {
                        match event {
                            CentralEvent::DeviceDiscovered(v) => self.on_device_discovered(v).await,
                            CentralEvent::DeviceUpdated(v) => self.on_device_updated(v).await,
                            CentralEvent::DeviceConnected(v) => self.on_device_connected(v).await,
                            CentralEvent::DeviceDisconnected(v) => self.on_device_disconnected(v).await,
                            _ => {},
                        }

                        let timeout_reached = self.config
                            .timeout
                            .filter(|timeout| Instant::now().duration_since(start_time).ge(timeout))
                            .is_some();

                        let max_result_reached = self.config
                            .max_results
                            .filter(|max_results| self.result_count >= *max_results)
                            .is_some();

                        if timeout_reached
                            || max_result_reached
                            || self.stopper.load(Ordering::Relaxed) {
                            log::info!("Scanner stop condition reached.");
                            return;
                        }
                    }
                }
            },
            Err(e) => log::warn!("Error: `{:?}` when start scan!", e),
        }
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

    async fn on_device_connected(&mut self, peripheral_id: PeripheralId) {
        self.connecting.lock().unwrap().remove(&peripheral_id);

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
    }

    async fn on_device_disconnected(&self, peripheral_id: PeripheralId) {
        if let Ok(peripheral) = self.session.adapter.peripheral(&peripheral_id).await {
            log::trace!("Device disconnected: {:?}", peripheral);

            if self.matched.contains(&peripheral_id) {
                let address = peripheral.address();
                match self.event_sender.send(DeviceEvent::Disconnected(Device::new(
                    self.session.adapter.clone(),
                    peripheral,
                ))) {
                    Ok(value) => log::trace!("Sent device: {}, size: {}...", address, value),
                    Err(e) => log::warn!("Error: {:?} when Sending device: {}...", e, address),
                }
            }
        }

        self.connecting.lock().unwrap().remove(&peripheral_id);
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
                    match filter {
                        Filter::Name(v) => {
                            if let Some(name_filter) = &self.config.name_filter {
                                passed &= name_filter(v);
                            }
                            else {
                                passed &= property
                                    .local_name
                                    .clone()
                                    .is_some_and(|name| &name == v);
                            }
                        }
                        Filter::Rssi(v) => {
                            if let Some(rssi_filter) = &self.config.rssi_filter {
                                passed &= rssi_filter(*v);
                            }
                            else {
                                passed &= property.rssi
                                    .is_some_and(|rssi| rssi >= *v);
                            }
                        }
                        Filter::Service(v) => {
                            if let Some(service_filter) = &self.config.service_filter {
                                passed &= service_filter(v);
                            }
                            else {
                                passed &= property.services.contains(v);
                            }
                        }
                        Filter::Address(v) => {
                            if let Some(address_filter) = &self.config.address_filter {
                                passed &= address_filter(*v);
                            }
                            else {
                                passed &= property.address == *v;
                            }
                        }
                        Filter::Characteristic(v) => {
                            self.apply_character_filter(&peripheral, v, &mut passed).await;
                        }
                    }
                }

                if passed {
                    self.matched.insert(peripheral_id.clone());
                    self.result_count += 1;

                    if let Err(e) = self.event_sender.send(
                        DeviceEvent::Discovered(
                            Device::new(self.session.adapter.clone(), peripheral))
                    ) {
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

    /// TODO validate
    async fn apply_character_filter(&self, peripheral: &Peripheral, uuid: &Uuid, passed: &mut bool) {
        if !peripheral.is_connected().await.unwrap_or(false) {
            if self.connecting.lock().unwrap().insert(peripheral.id()) {
                log::debug!("Connecting to device {}", peripheral.address());

                // Connect in another thread, so we can keep filtering other devices meanwhile.
                // let peripheral_clone = peripheral.clone();
                let connecting_map = self.connecting.clone();
                if let Err(e) = peripheral.connect().await {
                    log::warn!(
                            "Could not connect to {}: {:?}",
                            peripheral.address(),
                            e
                        );

                    connecting_map
                        .lock()
                        .unwrap()
                        .remove(&peripheral.id());

                    return;
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
                        characteristics_filter(characteristics.as_slice())
                    }
                    else {
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
    }
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::time::Duration;
    use std::vec;
    use btleplug::api::BDAddr;
    use btleplug::Error;
    use futures::StreamExt;
    use uuid::Uuid;
    use crate::Device;
    use super::{Filter, Scanner, ScanConfig};

    async fn device_stream<T: Future<Output = ()>>(scanner: Scanner, callback: impl Fn(Device) -> T) {
        let duration = Duration::from_millis(15_000);
        if let Err(_) = tokio::time::timeout(duration, async move {
            while let Some(device) = scanner.device_stream().next().await {
                callback(device).await;
                break;
            }
        }).await {
            eprintln!("timeout....");
        }
    }

    #[tokio::test]
    async fn test_filter_by_address() -> Result<(), Error> {
        pretty_env_logger::init();

        let mac_addr = [0xE3, 0x9E, 0x2A, 0x4D, 0xAA, 0x97];
        let filers = vec![
            Filter::Address(BDAddr::from(mac_addr.clone())),
        ];
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

        let filers = vec![
            Filter::Characteristic(Uuid::from_u128(0x6e400001_b5a3_f393_e0a9_e50e24dcca9e)),
        ];
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
        let filers = vec![
            Filter::Name(name.into()),
        ];
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

        let filers = vec![
            Filter::Rssi(-70),
        ];
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
        let filers = vec![
            Filter::Service(service),
        ];
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
