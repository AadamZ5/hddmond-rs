use std::{
    cell::RefCell,
    collections::{HashSet, VecDeque},
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::Poll,
    time::Duration,
    vec,
};

use anyhow::{Error, Ok};
use deno_core::futures::FutureExt;
use smartctl_wrapper::SmartCtl;
use tokio::{
    task::JoinHandle,
    time::{Instant, Sleep},
};
use tokio_stream::Stream;

use super::scanner::{DeviceMonitor, ScanEventType};

// SmartCtlMonitor will poll the `smartctl` binary with `--scan` to
// watch the list of devices. Unfortunately, this will not detect
// USB devices, or any device that isn't a SMART / SCSI / ATA type
// device. Using this implementation will effectively remove USB
// device functionality.
//
// Not to mention, this implementation is so crappy since I'm a
// beginner :)
pub struct SmartCtlMonitor {
    smartctl_bin_ref: Arc<SmartCtl>,
}

impl SmartCtlMonitor {
    pub fn new(smart_ctl_bin_ref: Option<SmartCtl>) -> Result<Self, Error> {
        let smart_ctl_bin_ref = match smart_ctl_bin_ref {
            Some(smart_ctl_bin_ref) => smart_ctl_bin_ref,
            None => SmartCtl::new(None)?,
        };

        Ok(Self {
            smartctl_bin_ref: Arc::new(smart_ctl_bin_ref),
        })
    }
}

impl DeviceMonitor for SmartCtlMonitor {
    fn watch_events(&self) -> Result<super::scanner::DeviceStream, Error> {
        let duration = Duration::from_secs(1);
        let sleep = tokio::time::sleep(duration.clone());

        let smartctl_bin_ref = self.smartctl_bin_ref.clone();

        let current_dev_names = Arc::new(std::sync::Mutex::new(HashSet::new()));

        Ok(Box::pin(SmartCtlMonitorStream {
            sleep_future: Rc::new(RefCell::new(Box::pin(sleep))),
            smartctl_bin_ref,
            smartctl_exec_fut: Rc::new(RefCell::new(None)),
            poll_interval: duration,
            current_dev_names,
            event_queue: VecDeque::new(),
        }))
    }
}

type SharedJoinHandle<T> = Rc<RefCell<Option<JoinHandle<T>>>>;

#[derive(Debug, Clone)]
pub struct SmartCtlDeviceListDiffResult {
    added: Vec<String>,
    removed: Vec<String>,
}

impl Default for SmartCtlDeviceListDiffResult {
    fn default() -> Self {
        Self {
            added: vec![],
            removed: vec![],
        }
    }
}

pub struct SmartCtlMonitorStream {
    smartctl_bin_ref: Arc<SmartCtl>,
    sleep_future: Rc<RefCell<Pin<Box<Sleep>>>>,
    smartctl_exec_fut: SharedJoinHandle<SmartCtlDeviceListDiffResult>,
    current_dev_names: Arc<std::sync::Mutex<HashSet<String>>>,
    poll_interval: Duration,
    event_queue: VecDeque<ScanEventType>,
}

impl SmartCtlMonitorStream {
    fn _upsert_smartctl_exec_future(
        &mut self,
    ) -> Result<SharedJoinHandle<SmartCtlDeviceListDiffResult>, Error> {
        let smartctl_ref = self.smartctl_bin_ref.clone();

        let mut current_fut = self.smartctl_exec_fut.as_ref().borrow_mut();

        if let Some(fut) = current_fut.as_mut() {
            drop(fut);
            drop(current_fut);
            return Ok(self.smartctl_exec_fut.clone());
        }

        let current_dev_names = self.current_dev_names.clone();

        let new_future = tokio::task::spawn_blocking(move || {
            let mut current_dev_names = current_dev_names.lock().unwrap();

            trace!("Scanning for devices...");

            let device_names = smartctl_ref.scan().unwrap_or(vec![]);

            // Get difference in device names
            let mut new_device_names = vec![];
            for device_name in device_names.clone() {
                if !current_dev_names.contains(&device_name) {
                    new_device_names.push(device_name);
                }
            }

            // Get missing device names
            let mut missing_device_names = vec![];
            for device_name in current_dev_names.iter() {
                if !device_names.contains(device_name) {
                    missing_device_names.push(device_name.clone());
                }
            }

            current_dev_names.extend(new_device_names.clone());
            for device_name in missing_device_names.iter() {
                current_dev_names.remove(device_name);
            }

            drop(current_dev_names);

            SmartCtlDeviceListDiffResult {
                added: new_device_names,
                removed: missing_device_names,
            }
        });

        current_fut.replace(new_future);

        Ok(self.smartctl_exec_fut.clone())
    }
}

impl Stream for SmartCtlMonitorStream {
    type Item = ScanEventType;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if let Some(event) = self.event_queue.pop_front() {
            cx.waker().wake_by_ref();
            return Poll::Ready(Some(event));
        }

        let sleep_fut_pointer = self.sleep_future.clone();
        let mut sleep_future = RefCell::borrow_mut(&sleep_fut_pointer);
        let interval_result = sleep_future.as_mut().poll(cx);

        if Poll::Pending == interval_result {
            return Poll::Pending;
        }

        let smartctl_exec_fut = self._upsert_smartctl_exec_future();

        // If we error, return None to signify an end of stream.
        if let Err(_) = smartctl_exec_fut {
            return Poll::Ready(None);
        }

        let fut_opt = smartctl_exec_fut.unwrap();
        let mut fut_opt = RefCell::borrow_mut(&fut_opt);

        if fut_opt.is_none() {
            let next_instant = Instant::now() + self.poll_interval.clone();
            sleep_future.as_mut().reset(next_instant);
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let fut = fut_opt.as_mut().unwrap();

        let result = fut.poll_unpin(cx);

        let poll_result = match result {
            Poll::Ready(r) => {
                if r.is_err() {
                    return Poll::Ready(None);
                }

                let next_instant = Instant::now() + self.poll_interval.clone();
                cx.waker().wake_by_ref();
                sleep_future.as_mut().reset(next_instant);
                trace!("Reset interval timer");

                // Set the future option to none so we can spawn a new one
                fut_opt.take();

                let r = r.unwrap_or(SmartCtlDeviceListDiffResult::default());

                for dev_name in r.added.iter() {
                    self.event_queue
                        .push_back(ScanEventType::DeviceFound(dev_name.clone()));
                }
                for dev_name in r.removed.iter() {
                    self.event_queue
                        .push_back(ScanEventType::DeviceLost(dev_name.clone()));
                }

                match self.event_queue.pop_front() {
                    Some(event) => Poll::Ready(Some(event)),
                    None => Poll::Pending,
                }
            }
            Poll::Pending => Poll::Pending,
        };

        return poll_result;
    }
}
