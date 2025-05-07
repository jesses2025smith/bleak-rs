use btleplug::{
    api::{BDAddr, Characteristic as BleCharacteristic, Peripheral as _, Service},
    platform::{Adapter, Peripheral},
    Result,
};
use std::collections::BTreeSet;
use uuid::Uuid;

use crate::Characteristic;

#[derive(Debug, Clone)]
pub struct Device {
    pub(self) _adapter: Adapter,
    pub(crate) peripheral: Peripheral,
}

impl Device {
    pub(crate) fn new(adapter: Adapter, peripheral: Peripheral) -> Self {
        Self {
            _adapter: adapter,
            peripheral,
        }
    }

    #[inline]
    pub fn address(&self) -> BDAddr {
        self.peripheral.address()
    }

    /// Signal strength
    #[inline]
    pub async fn rssi(&self) -> Option<i16> {
        self.peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|props| props.rssi)
    }

    /// Local name of the device
    #[inline]
    pub async fn local_name(&self) -> Option<String> {
        self.peripheral
            .properties()
            .await
            .ok()
            .flatten()
            .and_then(|props| props.local_name)
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
