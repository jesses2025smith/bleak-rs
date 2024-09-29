//! This example finds a BLE device with specified address.
//! cargo run --example filter_by_address XX:XX:XX:XX:XX:XX

use std::str::FromStr;
use btleplug::api::BDAddr;
use bleasy::{Error, Filter, ScanConfig, Scanner};
use futures::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Error> {
    let address = std::env::args()
        .nth(1)
        .expect("Expected address in format XX:XX:XX:XX:XX:XX");

    pretty_env_logger::init();

    log::info!("Scanning for device {}", address);

    let config = ScanConfig::default()
        .with_filters(&vec![
            Filter::Address(BDAddr::from_str(&address)?),
        ])
        .filter_by_address(move |addr| addr.to_string().eq(&address))
        .stop_after_first_match();

    let mut scanner = Scanner::new();
    scanner.start(config).await?;

    let device = scanner.device_stream().next().await;

    println!("{:?}", device.unwrap().local_name().await);

    Ok(())
}
