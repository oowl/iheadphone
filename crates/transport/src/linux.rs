//! Linux RFCOMM transport through BlueZ/BlueR.
//!
//! The connection pattern is adapted from `aecca/sound-connect` (MIT), while
//! the protocol driver is maintained locally by iHeadphone.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use bluer::rfcomm::{Profile, ProfileHandle, ReqError, Role, Stream};
use bluer::{Device, Uuid};
use futures::{FutureExt, StreamExt};
use iheadphone_sony_protocol::{Frame, Output, SERVICE_UUID, SonyXm5Driver};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tokio::time;

use crate::{Device as DeviceKind, Event, Request};

mod airpods;
mod l2cap;
mod media_player;

const RETRY_CONNECT: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(1);
const REFRESH_EVERY: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_millis(100);

pub fn start(
    name_filter: Option<String>,
    requests: mpsc::Receiver<Request>,
    events: async_channel::Sender<Event>,
) {
    let (sony_tx, sony_rx) = mpsc::channel();
    let (airpods_tx, airpods_rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("headphone-requests".into())
        .spawn(move || {
            while let Ok(request) = requests.recv() {
                let delivered = match request {
                    Request::Sony(setting) => sony_tx.send(setting).is_ok(),
                    Request::AirPods(setting) => airpods_tx.send(setting).is_ok(),
                };
                if !delivered {
                    break;
                }
            }
        })
        .expect("failed to start Bluetooth request router");

    let sony_events = events.clone();
    std::thread::Builder::new()
        .name("sony-bluetooth".into())
        .spawn(move || run_sony(name_filter, sony_rx, sony_events))
        .expect("failed to start Sony Bluetooth worker");
    std::thread::Builder::new()
        .name("airpods-bluetooth".into())
        .spawn(move || airpods::run(airpods_rx, events))
        .expect("failed to start AirPods Bluetooth worker");
    std::thread::Builder::new()
        .name("headset-buttons".into())
        .spawn(media_player::run)
        .expect("failed to start headset button player");
}

fn run_sony(
    name_filter: Option<String>,
    requests: mpsc::Receiver<iheadphone_sony_protocol::Setting>,
    events: async_channel::Sender<Event>,
) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to start Bluetooth runtime");
    runtime.block_on(link(name_filter, forward(requests), events));
}

fn forward(
    requests: mpsc::Receiver<iheadphone_sony_protocol::Setting>,
) -> UnboundedReceiver<iheadphone_sony_protocol::Setting> {
    let (sender, receiver) = unbounded_channel();
    std::thread::Builder::new()
        .name("sony-requests".into())
        .spawn(move || {
            while let Ok(request) = requests.recv() {
                if sender.send(request).is_err() {
                    break;
                }
            }
        })
        .expect("failed to start Bluetooth request forwarder");
    receiver
}

async fn link(
    name_filter: Option<String>,
    mut requests: UnboundedReceiver<iheadphone_sony_protocol::Setting>,
    events: async_channel::Sender<Event>,
) {
    let mut bluez = None;
    let mut searching = false;
    let mut failures = 0usize;

    loop {
        if bluez.is_none() {
            bluez = bluer::Session::new()
                .await
                .inspect_err(|error| eprintln!("bluetooth: cannot reach BlueZ: {error}"))
                .ok();
        }

        let channel = match &bluez {
            Some(bluez) => connect(bluez, name_filter.as_deref()).await,
            None => None,
        };
        if let Some(mut channel) = channel {
            searching = false;
            failures = 0;
            let _ = events
                .send(Event::Connected {
                    device: DeviceKind::SonyWh1000Xm5,
                    name: channel.name.clone(),
                })
                .await;
            let keep_running = serve(&mut channel.stream, &mut requests, &events).await;
            channel.close().await;
            let _ = events
                .send(Event::Disconnected(DeviceKind::SonyWh1000Xm5))
                .await;
            if !keep_running {
                return;
            }
            if events.is_closed() || requests.is_closed() {
                return;
            }
            // Retry immediately after a live link drops. If the headset is
            // still offline, the short backoff below takes over; if it has
            // already rebooted, this avoids an unconditional multi-second
            // pause before the first reconnect attempt.
            continue;
        } else {
            failures = failures.saturating_add(1);
            if !searching {
                searching = true;
                let _ = events
                    .send(Event::Searching(DeviceKind::SonyWh1000Xm5))
                    .await;
            }
        }

        if events.is_closed() || requests.is_closed() {
            return;
        }
        let delay = retry_delay(failures);
        time::sleep(delay).await;
    }
}

fn retry_delay(failures: usize) -> Duration {
    RETRY_CONNECT[failures.saturating_sub(1).min(RETRY_CONNECT.len() - 1)]
}

async fn serve(
    stream: &mut Stream,
    requests: &mut UnboundedReceiver<iheadphone_sony_protocol::Setting>,
    events: &async_channel::Sender<Event>,
) -> bool {
    let mut driver = SonyXm5Driver::default();
    let mut frames = deliver(driver.start(Instant::now()), events).await;
    let mut tick = time::interval(TICK);
    let mut refresh = time::interval_at(time::Instant::now() + REFRESH_EVERY, REFRESH_EVERY);
    let mut buffer = [0u8; 1024];

    loop {
        if write(stream, frames).await.is_err() {
            return true;
        }
        frames = tokio::select! {
            read = stream.read(&mut buffer) => {
                let Ok(count @ 1..) = read else { return true };
                if std::env::var_os("IHEADPHONE_TRACE_SONY").is_some() {
                    eprintln!("bluetooth: Sony RX {:02x?}", &buffer[..count]);
                }
                deliver(driver.receive(&buffer[..count], Instant::now()), events).await
            }
            request = requests.recv() => match request {
                Some(setting) => deliver(driver.set(setting, Instant::now()), events).await,
                None => return false,
            },
            _ = tick.tick() => deliver(driver.tick(Instant::now()), events).await,
            _ = refresh.tick() => deliver(driver.refresh(Instant::now()), events).await,
        };
    }
}

async fn deliver(output: Output, events: &async_channel::Sender<Event>) -> Vec<Frame> {
    for error in output.errors {
        eprintln!("bluetooth: discarded malformed Sony frame: {error:?}");
    }
    for update in output.updates {
        let _ = events.send(Event::SonyUpdate(update)).await;
    }
    output.writes
}

async fn write(stream: &mut Stream, frames: Vec<Frame>) -> std::io::Result<()> {
    for frame in frames {
        stream.write_all(&frame).await?;
    }
    Ok(())
}

struct Headset {
    device: Device,
    name: String,
    service: Uuid,
}

impl Headset {
    async fn check(
        device: Device,
        service: Uuid,
        name_filter: Option<&str>,
    ) -> bluer::Result<Option<Self>> {
        if !device.is_paired().await? || !device.is_connected().await? {
            return Ok(None);
        }
        let name = device.alias().await?;
        if name_filter.is_some_and(|filter| !name.contains(filter)) {
            return Ok(None);
        }
        let services = device.uuids().await?.unwrap_or_default();
        Ok(services.contains(&service).then_some(Self {
            device,
            name,
            service,
        }))
    }
}

struct Channel {
    device: Device,
    service: Uuid,
    name: String,
    stream: Stream,
    _profile: ProfileHandle,
}

impl Channel {
    async fn close(self) {
        let _ = time::timeout(
            DISCONNECT_TIMEOUT,
            self.device.disconnect_profile(&self.service),
        )
        .await;
    }
}

async fn connect(bluez: &bluer::Session, name_filter: Option<&str>) -> Option<Channel> {
    let service = Uuid::parse_str(SERVICE_UUID).ok()?;
    for device in devices(bluez).await {
        let Ok(Some(headset)) = Headset::check(device, service, name_filter).await else {
            continue;
        };
        let profile = Profile {
            uuid: service,
            name: Some("iHeadphone Sony Control".into()),
            role: Some(Role::Client),
            auto_connect: Some(false),
            ..Profile::default()
        };
        let Ok(mut profile) = bluez.register_profile(profile).await else {
            continue;
        };
        match open(&headset, &mut profile).await {
            Ok(stream) => {
                return Some(Channel {
                    device: headset.device,
                    service,
                    name: headset.name,
                    stream,
                    _profile: profile,
                });
            }
            Err(error) => {
                eprintln!(
                    "bluetooth: cannot open {} control channel: {error}",
                    headset.name
                );
                let _ = time::timeout(
                    DISCONNECT_TIMEOUT,
                    headset.device.disconnect_profile(&service),
                )
                .await;
            }
        }
    }
    None
}

async fn devices(bluez: &bluer::Session) -> Vec<Device> {
    let mut devices = Vec::new();
    for adapter_name in bluez.adapter_names().await.unwrap_or_default() {
        let Ok(adapter) = bluez.adapter(&adapter_name) else {
            continue;
        };
        for address in adapter.device_addresses().await.unwrap_or_default() {
            if let Ok(device) = adapter.device(address) {
                devices.push(device);
            }
        }
    }
    devices
}

async fn open(
    headset: &Headset,
    profile: &mut ProfileHandle,
) -> Result<Stream, Box<dyn std::error::Error>> {
    while let Some(Some(request)) = profile.next().now_or_never() {
        request.reject(ReqError::Canceled);
    }
    let address = headset.device.address();
    let accept = async {
        while let Some(request) = profile.next().await {
            if request.device() == address {
                return request.accept();
            }
            request.reject(ReqError::Rejected);
        }
        Err(bluer::Error {
            kind: bluer::ErrorKind::Failed,
            message: "profile was unregistered".into(),
        })
    };
    let connect = headset.device.connect_profile(&headset.service);
    let (_, stream) =
        time::timeout(CONNECT_TIMEOUT, async { tokio::try_join!(connect, accept) }).await??;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sony_service_uuid_is_valid() {
        assert!(Uuid::parse_str(SERVICE_UUID).is_ok());
    }

    #[test]
    fn reconnect_backoff_stays_responsive() {
        assert_eq!(retry_delay(1), Duration::from_millis(250));
        assert_eq!(retry_delay(2), Duration::from_millis(500));
        assert_eq!(retry_delay(3), Duration::from_secs(1));
        assert_eq!(retry_delay(20), Duration::from_secs(2));
    }
}
