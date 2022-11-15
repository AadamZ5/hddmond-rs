use anyhow::Error;
use log::warn;
use std::{future::Future, pin::Pin, rc::Rc, task::Poll, time::Duration};
use tokio::time::{interval, sleep, Interval, Sleep};
use tokio_stream::Stream;
use udev;

#[derive(Debug, Clone)]
pub enum ScanEvent {
    DeviceFound(String),
    DeviceLost(String),
    Unknown(String),
}

pub type DeviceStream = Pin<Box<dyn Stream<Item = ScanEvent>>>;

pub trait DeviceMonitor {
    // Function that takes a callback
    fn watch_events(&self) -> Result<DeviceStream, Error>;
}

pub struct UdevMonitor {
    udev_socket: Rc<udev::MonitorSocket>,
}

impl UdevMonitor {
    pub fn new() -> Result<Self, Error> {
        let udev_socket = udev::MonitorBuilder::new()?
            .match_subsystem("block")?
            .match_subsystem("scsi")?
            .listen()?;

        Ok(Self {
            udev_socket: Rc::new(udev_socket),
        })
    }
}

pub struct UdevMonitorStream {
    udev_socket: Rc<udev::MonitorSocket>,
    interval_future: Interval,
}

impl Stream for UdevMonitorStream {
    type Item = ScanEvent;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        // First, see if we still need to wait on our interval.
        // Pass the context to our interval and see our result.

        // If the interval has elapsed, do the actual check on the
        // udev socket. If we get a device, return it. If we get
        // None, then we need to wait on the interval again.

        let interval_result = self.interval_future.poll_tick(cx);

        // If the interval is still waiting, return now.
        // Do not worry about alerting the waker, as the interval will do that for us.
        if Poll::Pending == interval_result {
            return Poll::Pending;
        }

        // This iterator is non-blocking, and will return Some even
        // if it has once returned None.
        let event = self.udev_socket.iter().next();

        let result = if event.is_none() {
            Poll::Pending
        } else {
            trace!("New matched event: {:?}", event);

            let result = if let Some(event) = event {
                let device_name = event.device().sysname().to_str().map(|s| s.to_string());

                let direction = event
                    .action()
                    .map(|a| a.to_str())
                    .flatten()
                    .map(|a| match a {
                        "add" => ScanEvent::DeviceFound,
                        "remove" => ScanEvent::DeviceLost,
                        _ => ScanEvent::Unknown,
                    });

                if let (Some(direction), Some(device_name)) = (direction, device_name) {
                    Poll::Ready(Some(direction(device_name)))
                } else {
                    Poll::Pending
                }
            } else {
                Poll::Pending
            };

            result
        };

        // Since we didn't early escape and actually polled the udev
        // socket, our interval is now reset, and possibly didn't alert the
        // waker to wake this task. I do it here for good measure.
        cx.waker().wake_by_ref();

        result
    }
}

impl DeviceMonitor for UdevMonitor {
    fn watch_events(&self) -> Result<DeviceStream, Error> {
        // Interval determines how long to wait before polling the udev socket
        // again after a non-block / no-data event.
        let mut interval = interval(Duration::from_millis(100));

        // When the interval misses it's last tick (if we took too long to poll
        // or compute) the next tick will be immediate. After that next tick, the
        // interval will be normal again. This is desired.
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        Ok(Box::pin(UdevMonitorStream {
            udev_socket: self.udev_socket.clone(),
            interval_future: interval,
        }))
    }
}
