//! Keeps platform Bluetooth control links to supported headphones.

use std::sync::mpsc;

use iheadphone_airpods_protocol::{Setting as AirPodsSetting, Update as AirPodsUpdate};
use iheadphone_sony_protocol::{Setting as SonySetting, Update as SonyUpdate};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    SonyWh1000Xm5,
    AirPods,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Searching(Device),
    /// The operating system already has the headset connected, while the
    /// vendor control channel may still be opening in the background.
    Detected {
        device: Device,
        name: String,
    },
    Connected {
        device: Device,
        name: String,
    },
    SonyUpdate(SonyUpdate),
    AirPodsUpdate(AirPodsUpdate),
    Disconnected(Device),
    Unsupported {
        device: Device,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Sony(SonySetting),
    AirPods(AirPodsSetting),
}

pub struct Link {
    pub requests: mpsc::Sender<Request>,
    pub events: async_channel::Receiver<Event>,
}

pub fn start(name_filter: Option<String>) -> Link {
    let (request_tx, request_rx) = mpsc::channel();
    let (event_tx, events) = async_channel::unbounded();
    #[cfg(target_os = "linux")]
    linux::start(name_filter, request_rx, event_tx);
    #[cfg(target_os = "macos")]
    spawn_worker("headphone-bluetooth", move || {
        macos::run(name_filter, request_rx, event_tx);
    });
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    spawn_worker("headphone-bluetooth", move || {
        unsupported::run(name_filter, request_rx, event_tx);
    });
    Link {
        requests: request_tx,
        events,
    }
}

#[cfg(not(target_os = "linux"))]
fn spawn_worker(name: &str, worker: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name(name.into())
        .spawn(worker)
        .expect("failed to start Bluetooth worker");
}
