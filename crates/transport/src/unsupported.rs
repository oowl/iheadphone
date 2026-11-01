use std::sync::mpsc;

use crate::{Device, Event, Request};

pub fn run(
    _name_filter: Option<String>,
    _requests: mpsc::Receiver<Request>,
    events: async_channel::Sender<Event>,
) {
    let _ = events.send_blocking(Event::Unsupported {
        device: Device::SonyWh1000Xm5,
        reason: "This platform does not provide a Bluetooth control transport yet.".into(),
    });
}
