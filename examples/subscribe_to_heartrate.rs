//! This example finds the first BLE device that has a heart rate measurement characteristic,
//! connects to it and starts listening for heart rate values.

use bleasy::{
    characteristics::HEART_RATE_MEASUREMENT,
    {Filter, ScanConfig, Scanner},
};
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    rsutil::log::Log4rsConfig::default().initialize().unwrap();

    let config = ScanConfig::default()
        .with_filters(&vec![Filter::Characteristic(HEART_RATE_MEASUREMENT)])
        .filter_by_characteristics(|chars, hrm| chars.contains(hrm))
        .stop_after_first_match();

    let mut scanner = Scanner::new();
    scanner.start(config).await?;

    let mut device_stream = scanner.device_stream()?;
    let device = device_stream.next().await.unwrap();

    scanner.stop().await?;

    for service in device.services().await? {
        println!("Service: {:?}", service);
    }

    let hr_measurement = device
        .characteristic(HEART_RATE_MEASUREMENT)
        .await?
        .unwrap();
    let mut hr_stream = hr_measurement.subscribe().await?;

    while let Some(hr) = hr_stream.next().await {
        println!("RSSI: {}", device.rssi().await.unwrap_or(0));
        println!("{:?}", hr);
    }

    Ok(())
}
