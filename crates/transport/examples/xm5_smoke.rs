//! Reversible real-hardware smoke test for the Sony write path.

use std::thread;
use std::time::{Duration, Instant};

use iheadphone_sony_protocol::{Feature, Setting, Update};
use iheadphone_transport::{Event, Request};

fn wait_for_dsee(
    events: &async_channel::Receiver<Event>,
    expected: Option<bool>,
    timeout: Duration,
) -> Option<bool> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        match events.try_recv() {
            Ok(Event::SonyUpdate(Update::Feature(Feature::Dsee, enabled)))
                if expected.is_none_or(|expected| expected == enabled) =>
            {
                return Some(enabled);
            }
            Ok(_) | Err(async_channel::TryRecvError::Empty) => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(async_channel::TryRecvError::Closed) => return None,
        }
    }
    None
}

fn main() {
    let link = iheadphone_transport::start(Some("WH-1000XM5".into()));
    let original = wait_for_dsee(&link.events, None, Duration::from_secs(20))
        .expect("XM5 did not report DSEE within 20 seconds");
    let changed = !original;

    link.requests
        .send(Request::Sony(Setting::Feature(Feature::Dsee, changed)))
        .expect("Bluetooth worker stopped before the change");
    wait_for_dsee(&link.events, Some(changed), Duration::from_secs(5))
        .expect("XM5 did not confirm the temporary DSEE change");

    link.requests
        .send(Request::Sony(Setting::Feature(Feature::Dsee, original)))
        .expect("Bluetooth worker stopped before restoring DSEE");
    wait_for_dsee(&link.events, Some(original), Duration::from_secs(5))
        .expect("XM5 did not confirm DSEE was restored");

    println!("XM5 write path verified; DSEE restored to {original}");
}
