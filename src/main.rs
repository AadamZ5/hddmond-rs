mod scanners;

#[macro_use]
extern crate log;

use anyhow::Error;
use scanners::{
    scanner::{DeviceMonitor, ScanEventType},
    smartctl_scanner::SmartCtlMonitor,
    udev_scanner::UdevMonitor,
};
use simple_logger::SimpleLogger;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Error> {
    info!("Starting...");

    SimpleLogger::new()
        .with_level(log::LevelFilter::Trace)
        .init()?;

    let monitor = UdevMonitor::new()?;

    info!("Created udev monitor.");

    let mut stream = monitor.watch_events()?;

    while let Some(event) = stream.next().await {
        match event {
            ScanEventType::DeviceFound(device) => {
                info!("Found device: {}", device);
            }
            ScanEventType::DeviceLost(device) => {
                info!("Lost device: {}", device);
            }
            ScanEventType::Unknown(device) => {
                info!("Unknown action for device: {}", device);
            }
        }
    }

    info!("Exiting...");

    Ok(())
}
