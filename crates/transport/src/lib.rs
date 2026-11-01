//! Keeps a platform RFCOMM link to a Sony WH-1000XM5 on a background thread.

use std::sync::mpsc;

use iheadphone_sony_protocol::{Setting, Update};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod unsupported;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Searching,
    Connected { name: String },
    Update(Update),
    Disconnected,
    Unsupported(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    Change(Setting),
}

pub struct Link {
    pub requests: mpsc::Sender<Request>,
    pub events: async_channel::Receiver<Event>,
}

pub fn start(name_filter: Option<String>) -> Link {
    let (request_tx, request_rx) = mpsc::channel();
    let (event_tx, events) = async_channel::unbounded();
    std::thread::Builder::new()
        .name("headphone-bluetooth".into())
        .spawn(move || {
            #[cfg(target_os = "linux")]
            linux::run(name_filter, request_rx, event_tx);
            #[cfg(target_os = "macos")]
            macos::run(name_filter, request_rx, event_tx);
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            unsupported::run(name_filter, request_rx, event_tx);
        })
        .expect("failed to start Bluetooth worker");
    Link {
        requests: request_tx,
        events,
    }
}
