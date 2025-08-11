use btleplug::{
    api::{
        BDAddr, Central as _, CentralEvent, Characteristic as BleCharacteristic, Peripheral as _,
        PeripheralProperties, Service,
    },
    platform::{Adapter, Peripheral, PeripheralId},
    Result,
};
use std::{collections::BTreeSet, sync::Arc, time::Duration};
use tokio::{task::JoinHandle, time};
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::Characteristic;

#[derive(Debug, Clone)]
pub struct Device {
    pub(self) _adapter: Adapter,
    pub(crate) peripheral: Peripheral,
    pub(crate) task: Arc<Option<JoinHandle<()>>>,
}

impl Device {
    pub(crate) fn new(adapter: Adapter, peripheral: Peripheral) -> Self {
        Self {
            _adapter: adapter,
            peripheral,
            task: Arc::new(None),
        }
    }

    #[inline]
    pub fn address(&self) -> BDAddr {
        self.peripheral.address()
    }

    /// add device disconnected callback
    pub fn on_disconnected<F>(&mut self, f: F)
    where
        F: FnOnce(PeripheralId) + Send + 'static,
    {
        let adapter_clone = self._adapter.clone();
        let peripheral_clone = self.peripheral.clone();
        let handle = tokio::spawn(async move {
            if let Ok(mut stream) = adapter_clone.events().await {
                while let Some(event) = stream.next().await {
                    match event {
                        CentralEvent::DeviceDisconnected(per) => {
                            if per == peripheral_clone.id() {
                                f(per);
                                break;
                            }
                        }
                        _ => time::sleep(Duration::from_micros(100)).await,
                    }
                }
            }
        });

        self.task = Arc::new(Some(handle));
    }

    /// Get the peripheral properties from device.
    pub async fn properties(&self) -> Option<PeripheralProperties> {
        self.peripheral.properties().await.ok().flatten()
    }

    /// Signal strength
    #[inline]
    pub async fn rssi(&self) -> Option<i16> {
        self.properties().await.map(|p| p.rssi).flatten()
    }

    /// Local name of the device
    #[inline]
    pub async fn local_name(&self) -> Option<String> {
        self.properties()
            .await
            .map(|props| props.local_name)
            .flatten()
    }

    /// Get the manufacturer data from device.
    pub async fn manufacturer_data(&self, val: &u16) -> Option<Vec<u8>> {
        self.properties()
            .await
            .map(|p| p.manufacturer_data.get(val).cloned())
            .flatten()
    }

    /// Get the service data from device.
    pub async fn service_data(&self, uuid: &Uuid) -> Option<Vec<u8>> {
        self.properties()
            .await
            .map(|p| p.service_data.get(uuid).cloned())
            .flatten()
    }

    /// Connect the device
    #[inline]
    pub async fn connect(&self) -> Result<()> {
        if !self.is_connected().await? {
            log::info!("Connecting device.");
            self.peripheral.connect().await?;
        }

        Ok(())
    }

    /// Disconnect from the device
    #[inline]
    pub async fn disconnect(&self) -> Result<()> {
        log::info!("Disconnecting device.");
        self.peripheral.disconnect().await
    }

    /// Get the connected state
    #[inline]
    pub async fn is_connected(&self) -> Result<bool> {
        self.peripheral.is_connected().await
    }

    /// Services advertised by the device
    pub async fn services(&self) -> Result<Vec<Service>> {
        // self.connect().await?;

        let mut services = self.peripheral.services();
        if services.is_empty() {
            self.peripheral.discover_services().await?;
            services = self.peripheral.services();
        }

        Ok(services.into_iter().collect::<Vec<_>>())
    }

    /// Number of services advertised by the device
    pub async fn service_count(&self) -> Result<usize> {
        Ok(self.services().await?.len())
    }

    /// Characteristics advertised by the device
    pub async fn characteristics(&self) -> Result<Vec<Characteristic>> {
        let characteristics = self.original_characteristics().await?;
        Ok(characteristics
            .into_iter()
            .map(|characteristic| Characteristic {
                peripheral: self.peripheral.clone(),
                characteristic,
            })
            .collect::<Vec<_>>())
    }

    /// Get characteristic by UUID
    pub async fn characteristic(&self, uuid: Uuid) -> Result<Option<Characteristic>> {
        let characteristics = self.original_characteristics().await?;

        Ok(characteristics
            .into_iter()
            .find(|characteristic| characteristic.uuid == uuid)
            .map(|characteristic| Characteristic {
                peripheral: self.peripheral.clone(),
                characteristic,
            }))
    }

    #[inline]
    async fn original_characteristics(&self) -> Result<BTreeSet<BleCharacteristic>> {
        // self.connect().await?;

        let mut characteristics = self.peripheral.characteristics();
        if characteristics.is_empty() {
            self.peripheral.discover_services().await?;
            characteristics = self.peripheral.characteristics();
        }

        Ok(characteristics)
    }
}

#[derive(Debug, Clone)]
pub enum DeviceEvent {
    Discovered(Device),
    Connected(Device),
    Disconnected(Device),
    Updated(Device),
}
