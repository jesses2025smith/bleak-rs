//! High-level BLE communication library.
//!
//! The goal of this library is to provide an easy-to-use interface
//! for communicating with BLE devices, that satisfies most use cases.
//!
//! ## Usage
//!
//! Here is an example on how to find a device with battery level characteristic and read
//! a value from that characteristic:
//!
//! ```rust,no_run
//! use bleasy::{characteristics::BATTERY_LEVEL, Error, Filter, ScanConfig, Scanner};
//! use tokio_stream::StreamExt;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Error> {
//!     rsutil::log::Log4rsConfig::default().initialize().unwrap();
//!
//!     // Create a filter for devices that have battery level characteristic
//!     let config = ScanConfig::default()
//!         .with_filters(&[Filter::Characteristic(BATTERY_LEVEL)])
//!         .filter_by_characteristics(|characters, bat_lvl| characters.contains(bat_lvl))
//!         .stop_after_first_match();
//!
//!     // Start scanning for devices
//!     let mut scanner = Scanner::new();
//!     scanner.start(config).await?;
//!
//!     // Take the first discovered device
//!     let device = scanner.device_stream().next().await.unwrap();
//!     println!("{:?}", device);
//!
//!     // Read the battery level
//!     let battery_level = device.characteristic(BATTERY_LEVEL).await?.unwrap();
//!     println!("Battery level: {:?}", battery_level.read().await?);
//!
//!     Ok(())
//! }
//!```

#![warn(clippy::all, future_incompatible, nonstandard_style, rust_2018_idioms)]

mod characteristic;
mod common;
mod device;
mod scanner;

pub use self::{
    characteristic::Characteristic,
    common::*,
    device::{Device, DeviceEvent},
    scanner::{
        config::{Filter, ScanConfig},
        Scanner,
    },
};
pub use btleplug::{
    api::{BDAddr, PeripheralProperties},
    Error, Result,
};

#[cfg(test)]
mod tests {
    use crate::{Filter, ScanConfig, Scanner};
    use btleplug::{api::BDAddr, Error};
    use std::time::Duration;
    use tokio_stream::StreamExt;
    use uuid::Uuid;

    #[tokio::test]
    async fn test_discover() -> anyhow::Result<()> {
        rsutil::log::Log4rsConfig::default().initialize().unwrap();

        let cfg = ScanConfig::default().stop_after_timeout(Duration::from_secs(10));

        let mut scanner = Scanner::new();
        scanner.start(cfg).await?;

        while let Some(device) = scanner.device_stream()?.next().await {
            println!("Found device: {}", device.address());
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_address() -> Result<(), Error> {
        rsutil::log::Log4rsConfig::default().initialize().unwrap();

        let mac_addr = [0xE3, 0x9E, 0x2A, 0x4D, 0xAA, 0x97];
        let filers = vec![Filter::Address("E3:9E:2A:4D:AA:97".into())];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_timeout(Duration::from_secs(10))
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        while let Some(device) = scanner.device_stream()?.next().await {
            assert_eq!(device.address(), BDAddr::from(mac_addr));
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_character() -> Result<(), Error> {
        rsutil::log::Log4rsConfig::default().initialize().unwrap();

        let filers = vec![Filter::Characteristic(Uuid::from_u128(
            0x6e400001_b5a3_f393_e0a9_e50e24dcca9e,
        ))];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_timeout(Duration::from_secs(10))
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        while let Some(device) = scanner.device_stream()?.next().await {
            println!("device: {:?} found", device);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_name() -> Result<(), Error> {
        rsutil::log::Log4rsConfig::default().initialize().unwrap();

        let name = "73429485";
        let filers = vec![Filter::Name(name.into())];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_timeout(Duration::from_secs(10))
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        while let Some(device) = scanner.device_stream()?.next().await {
            assert_eq!(device.local_name().await, Some(name.into()));
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_rssi() -> Result<(), Error> {
        rsutil::log::Log4rsConfig::default().initialize().unwrap();

        let filers = vec![Filter::Rssi(-70)];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_timeout(Duration::from_secs(10))
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        while let Some(device) = scanner.device_stream()?.next().await {
            println!("device: {:?} found", device);
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_filter_by_service() -> Result<(), Error> {
        rsutil::log::Log4rsConfig::default().initialize().unwrap();

        let service = Uuid::from_u128(0x6e400001_b5a3_f393_e0a9_e50e24dcca9e);
        let filers = vec![Filter::Service(service)];
        let cfg = ScanConfig::default()
            .with_filters(&filers)
            .stop_after_timeout(Duration::from_secs(10))
            .stop_after_first_match();
        let mut scanner = Scanner::default();

        scanner.start(cfg).await?;
        while let Some(device) = scanner.device_stream()?.next().await {
            println!("device: {:?} found", device);
        }

        Ok(())
    }
}
