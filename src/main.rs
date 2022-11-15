mod scanner;

#[macro_use]
extern crate log;

use anyhow::Error;
use scanner::{DeviceMonitor, UdevMonitor};
use simple_logger::SimpleLogger;
use tokio_stream::StreamExt;

#[tokio::main]
async fn main() -> Result<(), Error> {
    info!("Starting...");

    SimpleLogger::new()
        .with_level(log::LevelFilter::Debug)
        .init()?;

    let monitor = UdevMonitor::new()?;

    info!("Created udev monitor.");

    let mut stream = monitor.watch_events()?;

    while let Some(event) = stream.next().await {
        debug!("Got event: {:?}", event);
        match event {
            scanner::ScanEvent::DeviceFound(device) => {
                info!("Found device: {}", device);
            }
            scanner::ScanEvent::DeviceLost(device) => {
                info!("Lost device: {}", device);
            }
            scanner::ScanEvent::Unknown(device) => {
                info!("Unknown action for device: {}", device);
            }
        }
    }

    info!("Exiting...");

    Ok(())
}
