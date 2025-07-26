use std::time::Duration;
use uuid::Uuid;

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
    pub(crate) adapter_index: usize,
    /// Filters objects
    pub(crate) filters: Vec<Filter>,
    /// Filters the found devices based on device address.
    pub(crate) address_filter: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
    /// Filters the found devices based on local name.
    pub(crate) name_filter: Option<Box<dyn Fn(&str) -> bool + Send + Sync>>,
    /// Filters the found devices based on rssi.
    pub(crate) rssi_filter: Option<Box<dyn Fn(i16) -> bool + Send + Sync>>,
    /// Filters the found devices based on service's uuid.
    pub(crate) service_filter: Option<Box<dyn Fn(&Vec<Uuid>, &Uuid) -> bool + Send + Sync>>,
    /// Filters the found devices based on characteristics. Requires a connection to the device.
    pub(crate) characteristics_filter: Option<Box<dyn Fn(&Vec<Uuid>) -> bool + Send + Sync>>,
    /// Maximum results before the scan is stopped.
    pub(crate) max_results: Option<usize>,
    /// The scan is stopped when timeout duration is reached.
    pub(crate) timeout: Option<Duration>,
    /// Force disconnect when listen the device is connected.
    pub(crate) force_disconnect: bool,
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
