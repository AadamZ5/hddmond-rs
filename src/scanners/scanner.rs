use anyhow::Error;
use std::pin::Pin;
use tokio_stream::Stream;

#[derive(Debug, Clone)]
pub enum ScanEventType {
    DeviceFound(String),
    DeviceLost(String),
    Unknown(String),
}

pub type DeviceStream = Pin<Box<dyn Stream<Item = ScanEventType>>>;

pub trait DeviceMonitor {
    // Function that takes a callback
    fn watch_events(&self) -> Result<DeviceStream, Error>;
}
