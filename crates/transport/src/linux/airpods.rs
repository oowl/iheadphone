use std::collections::{HashMap, HashSet};
use std::io;
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bluer::{Adapter, AdapterEvent, Address, Device};
use futures::{
    StreamExt, pin_mut,
    stream::{BoxStream, SelectAll},
};
use iheadphone_airpods_protocol::{
    AAP_PSM, BatteryState, ProximityKeys, Setting, address_resolves, encode_setting,
    initialization_packets, parse_case_advertisement, parse_proximity_keys,
};

use super::l2cap;
use crate::{Device as DeviceKind, Event};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
];

pub fn run(requests: mpsc::Receiver<Setting>, events: async_channel::Sender<Event>) {
    let proximity_keys = Arc::new(Mutex::new(None));
    spawn_ble_scanner(Arc::clone(&proximity_keys), events.clone());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to start AirPods Bluetooth runtime");
    let mut failures = 0usize;
    let mut searching = false;
    loop {
        let candidate = runtime.block_on(find_connected());
        if let Some((device, name)) = candidate {
            match connect_with_retry(&device, &requests) {
                Ok(stream) => {
                    failures = 0;
                    searching = false;
                    let _ = events.send_blocking(Event::Connected {
                        device: DeviceKind::AirPods,
                        name,
                    });
                    if !serve(&stream, &requests, &events, &proximity_keys) {
                        return;
                    }
                    let _ = events.send_blocking(Event::Disconnected(DeviceKind::AirPods));
                    continue;
                }
                Err(error) => {
                    eprintln!("bluetooth: cannot open AirPods AAP channel: {error}");
                }
            }
        }
        failures = failures.saturating_add(1);
        if !searching {
            searching = true;
            let _ = events.send_blocking(Event::Searching(DeviceKind::AirPods));
        }
        match requests.recv_timeout(RETRY[(failures - 1).min(RETRY.len() - 1)]) {
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if events.is_closed() {
            return;
        }
    }
}

fn connect_with_retry(
    device: &Device,
    requests: &mpsc::Receiver<Setting>,
) -> io::Result<l2cap::Stream> {
    let mut last_error = None;
    for wait in [0, 300, 700, 1200, 2000] {
        if wait > 0 {
            match requests.recv_timeout(Duration::from_millis(wait)) {
                Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(io::ErrorKind::Interrupted, "app closed"));
                }
            }
        }
        match l2cap::Stream::connect(device.address(), AAP_PSM, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("AirPods connection failed")))
}

fn serve(
    stream: &l2cap::Stream,
    requests: &mpsc::Receiver<Setting>,
    events: &async_channel::Sender<Event>,
    proximity_keys: &Mutex<Option<ProximityKeys>>,
) -> bool {
    for packet in initialization_packets() {
        if stream.send(packet).is_err() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let mut buffer = [0u8; 2048];
    loop {
        loop {
            match requests.try_recv() {
                Ok(setting) => {
                    if stream.send(&encode_setting(setting)).is_err() {
                        return true;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
        match stream.receive(&mut buffer) {
            Ok(0) => return true,
            Ok(count) => {
                if std::env::var_os("IHEADPHONE_TRACE_AIRPODS").is_some() {
                    eprintln!("bluetooth: AirPods RX {:02x?}", &buffer[..count]);
                }
                if let Some(keys) = parse_proximity_keys(&buffer[..count]) {
                    if let Ok(mut stored) = proximity_keys.lock() {
                        *stored = Some(keys);
                    }
                    eprintln!("bluetooth: AirPods proximity keys received; case scanning enabled");
                }
                match iheadphone_airpods_protocol::parse(&buffer[..count]) {
                    Ok(updates) => {
                        for update in updates {
                            let _ = events.send_blocking(Event::AirPodsUpdate(update));
                        }
                    }
                    Err(error) => {
                        eprintln!("bluetooth: discarded malformed AirPods frame: {error:?}");
                    }
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return true,
        }
    }
}

fn spawn_ble_scanner(
    proximity_keys: Arc<Mutex<Option<ProximityKeys>>>,
    events: async_channel::Sender<Event>,
) {
    std::thread::Builder::new()
        .name("airpods-case-ble".into())
        .spawn(move || {
            let keys = loop {
                if events.is_closed() {
                    return;
                }
                if let Some(keys) = proximity_keys.lock().ok().and_then(|keys| *keys) {
                    break keys;
                }
                std::thread::sleep(Duration::from_millis(250));
            };
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to start AirPods BLE runtime");
            runtime.block_on(scan_case(keys, events));
        })
        .expect("failed to start AirPods case scanner");
}

async fn scan_case(keys: ProximityKeys, events: async_channel::Sender<Event>) {
    let mut last_case = None;
    loop {
        if let Err(error) = scan_case_once(&keys, &events, &mut last_case).await {
            eprintln!("bluetooth: AirPods case scan stopped: {error}");
        }
        if events.is_closed() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn scan_case_once(
    keys: &ProximityKeys,
    events: &async_channel::Sender<Event>,
    last_case: &mut Option<(u8, bool)>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Hold a dedicated D-Bus connection for the discovery lease. Using a
    // second bluer discovery session races its process-wide session guard on
    // BlueZ 5.87, even though a direct Adapter1.StartDiscovery succeeds.
    let bluez_connection = zbus::Connection::system().await?;
    let bluez_adapter = zbus::Proxy::new(
        &bluez_connection,
        "org.bluez",
        "/org/bluez/hci0",
        "org.bluez.Adapter1",
    )
    .await?;
    let session = bluer::Session::new().await?;
    let adapter = session.default_adapter().await?;
    let owns_discovery = if adapter.is_discovering().await? {
        false
    } else {
        let mut filter = HashMap::new();
        filter.insert("Transport", zbus::zvariant::Value::from("le"));
        filter.insert("DuplicateData", zbus::zvariant::Value::from(true));
        bluez_adapter
            .call_method("SetDiscoveryFilter", &(filter,))
            .await?;
        match bluez_adapter.call_method("StartDiscovery", &()).await {
            Ok(_) => true,
            Err(_error) if adapter.is_discovering().await.unwrap_or(false) => false,
            // BlueZ can report InProgress before its Discovering property has
            // propagated to this connection. The existing discovery session
            // is still usable: manufacturer-data changes are broadcast to all
            // clients, so subscribe below instead of tearing the scanner down.
            Err(error) if discovery_is_in_progress(&error) => false,
            Err(error) => return Err(error.into()),
        }
    };
    let mut changes = SelectAll::new();
    let mut watched = HashSet::new();
    for address in adapter.device_addresses().await? {
        watch_address(&adapter, address, &mut watched, &mut changes).await;
        publish_case(&adapter, address, keys, events, last_case).await;
    }
    let adapter_events = adapter.events().await?;
    pin_mut!(adapter_events);
    loop {
        tokio::select! {
            event = adapter_events.next() => {
                let Some(AdapterEvent::DeviceAdded(address)) = event else { continue };
                publish_case(&adapter, address, keys, events, last_case).await;
                watch_address(&adapter, address, &mut watched, &mut changes).await;
            }
            Some(address) = changes.next(), if !changes.is_empty() => {
                publish_case(&adapter, address, keys, events, last_case).await;
            }
            () = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
        if events.is_closed() {
            if owns_discovery {
                let _ = bluez_adapter.call_method("StopDiscovery", &()).await;
            }
            return Ok(());
        }
    }
}

fn discovery_is_in_progress(error: &zbus::Error) -> bool {
    matches!(
        error,
        zbus::Error::MethodError(name, _, _)
            if name.as_str() == "org.bluez.Error.InProgress"
    )
}

async fn watch_address(
    adapter: &Adapter,
    address: Address,
    watched: &mut HashSet<Address>,
    changes: &mut SelectAll<BoxStream<'static, Address>>,
) {
    if watched.insert(address)
        && let Ok(device) = adapter.device(address)
        && let Ok(stream) = device.events().await
    {
        changes.push(stream.map(move |_| address).boxed());
    }
}

async fn publish_case(
    adapter: &Adapter,
    address: Address,
    keys: &ProximityKeys,
    events: &async_channel::Sender<Event>,
    last_case: &mut Option<(u8, bool)>,
) {
    if !address_resolves(&keys.identity, address.0) {
        return;
    }
    let Ok(device) = adapter.device(address) else {
        return;
    };
    let Some(data) = device
        .manufacturer_data()
        .await
        .ok()
        .flatten()
        .and_then(|mut data| data.remove(&0x004c))
    else {
        return;
    };
    let Some(advertisement) = parse_case_advertisement(&data) else {
        return;
    };
    let Some(level) = advertisement.case_battery else {
        return;
    };
    let value = (level, advertisement.case_charging);
    if *last_case == Some(value) {
        return;
    }
    *last_case = Some(value);
    eprintln!("bluetooth: AirPods case battery {level}%");
    let _ = events
        .send(Event::AirPodsUpdate(
            iheadphone_airpods_protocol::Update::Battery(BatteryState {
                case: Some(level),
                case_charging: advertisement.case_charging,
                ..BatteryState::default()
            }),
        ))
        .await;
}

async fn find_connected() -> Option<(Device, String)> {
    let session = bluer::Session::new().await.ok()?;
    for device in super::devices(&session).await {
        if !matches!(device.is_paired().await, Ok(true))
            || !matches!(device.is_connected().await, Ok(true))
        {
            continue;
        }
        let Ok(name) = device.alias().await else {
            continue;
        };
        let named_airpods = name.to_ascii_lowercase().contains("airpods");
        let apple_headphones = device
            .modalias()
            .await
            .ok()
            .flatten()
            .is_some_and(|modalias| modalias.vendor == 0x004c)
            && device
                .icon()
                .await
                .ok()
                .flatten()
                .is_some_and(|icon| icon == "audio-headphones");
        if named_airpods || apple_headphones {
            return Some((device, name));
        }
    }
    None
}
