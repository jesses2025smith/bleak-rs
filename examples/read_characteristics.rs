//! This example finds the first device with battery level characteristic
//! and reads the device's battery level.

use bleasy::{
    common::characteristics::BATTERY_LEVEL,
    {Filter, ScanConfig, Scanner},
};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rsutil::log::Log4rsConfig::default().initialize().unwrap();

    // Filters devices that have battery level characteristic
    let config = ScanConfig::default()
        .with_filters(&vec![Filter::Characteristic(BATTERY_LEVEL)])
        .filter_by_characteristics(|uuids| uuids.contains(&BATTERY_LEVEL))
        .stop_after_first_match();

    // Start scanning for devices
    let mut scanner = Scanner::new();
    scanner.start(config).await?;

    // Take the first discovered device
    let device = scanner.device_stream()?.next().await.unwrap();

    // Read the battery level
    let battery_level = device.characteristic(BATTERY_LEVEL).await?.unwrap();
    println!("Battery level: {:?}", battery_level.read().await?);

    Ok(())
}
