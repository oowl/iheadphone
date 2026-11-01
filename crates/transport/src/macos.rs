//! macOS RFCOMM transport through IOBluetooth.
//!
//! The delegate/run-loop pattern is adapted from `aecca/sound-connect`
//! (MIT). Buffer bounds and callback pointer validation are local hardening.

use std::ffi::c_void;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use iheadphone_sony_protocol::{Frame, Output, SERVICE_UUID, SonyXm5Driver};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
use objc2_io_bluetooth::{
    IOBluetoothDevice, IOBluetoothRFCOMMChannel, IOBluetoothRFCOMMChannelDelegate,
    IOBluetoothSDPUUID, kBluetoothDeviceClassMajorAudio,
};

use crate::{Device as DeviceKind, Event, Request};

const RETRY_CONNECT: [Duration; 4] = [
    Duration::from_secs(3),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
];
const POLL: Duration = Duration::from_millis(20);
const REFRESH_EVERY: Duration = Duration::from_secs(60);
const SDP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_INCOMING: usize = 8 * 1024;

struct Connection {
    name: String,
    channel: Retained<IOBluetoothRFCOMMChannel>,
    driver: SonyXm5Driver,
}

pub fn run(
    name_filter: Option<String>,
    requests: mpsc::Receiver<Request>,
    events: async_channel::Sender<Event>,
) {
    let delegate = Delegate::new();
    let Some(service) = sdp_uuid(SERVICE_UUID) else {
        let _ = events.send_blocking(Event::Unsupported {
            device: DeviceKind::SonyWh1000Xm5,
            reason: "Invalid Sony service UUID".into(),
        });
        return;
    };
    let mut searching = false;
    let mut failures = 0usize;

    loop {
        if let Some(mut connection) = connect(&delegate, &service, name_filter.as_deref()) {
            searching = false;
            failures = 0;
            let _ = events.send_blocking(Event::Connected {
                device: DeviceKind::SonyWh1000Xm5,
                name: connection.name.clone(),
            });
            let keep_running = serve(&delegate, &mut connection, &requests, &events);
            unsafe { connection.channel.closeChannel() };
            let _ = events.send_blocking(Event::Disconnected(DeviceKind::SonyWh1000Xm5));
            if !keep_running {
                return;
            }
        } else {
            failures = failures.saturating_add(1);
            if !searching {
                searching = true;
                let _ = events.send_blocking(Event::Searching(DeviceKind::SonyWh1000Xm5));
            }
        }

        let deadline = Instant::now() + RETRY_CONNECT[failures.min(RETRY_CONNECT.len() - 1)];
        while Instant::now() < deadline {
            match requests.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        if events.is_closed() {
            return;
        }
    }
}

fn serve(
    delegate: &Delegate,
    connection: &mut Connection,
    requests: &mpsc::Receiver<Request>,
    events: &async_channel::Sender<Event>,
) -> bool {
    let Connection {
        channel, driver, ..
    } = connection;
    deliver(channel, events, driver.start(Instant::now()));
    let mut refreshed = Instant::now();

    loop {
        spin_run_loop(POLL);
        let now = Instant::now();
        if delegate.ivars().overflow.swap(false, Ordering::SeqCst) {
            eprintln!("bluetooth: discarded oversized IOBluetooth callback buffer");
        }
        let incoming = delegate
            .ivars()
            .incoming
            .lock()
            .map(|mut incoming| std::mem::take(&mut *incoming))
            .unwrap_or_default();
        if !incoming.is_empty() {
            deliver(channel, events, driver.receive(&incoming, now));
        }
        if delegate.ivars().closed.swap(false, Ordering::SeqCst) {
            return true;
        }

        match requests.recv_timeout(POLL) {
            Ok(Request::Sony(setting)) => deliver(channel, events, driver.set(setting, now)),
            Ok(Request::AirPods(_)) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return false,
        }
        deliver(channel, events, driver.tick(now));
        if now.saturating_duration_since(refreshed) >= REFRESH_EVERY {
            refreshed = now;
            deliver(channel, events, driver.refresh(now));
        }
    }
}

fn deliver(
    channel: &IOBluetoothRFCOMMChannel,
    events: &async_channel::Sender<Event>,
    output: Output,
) {
    write(channel, output.writes);
    for error in output.errors {
        eprintln!("bluetooth: discarded malformed Sony frame: {error:?}");
    }
    for update in output.updates {
        let _ = events.send_blocking(Event::SonyUpdate(update));
    }
}

fn write(channel: &IOBluetoothRFCOMMChannel, frames: Vec<Frame>) {
    for mut frame in frames {
        let Ok(length) = u16::try_from(frame.len()) else {
            continue;
        };
        unsafe { channel.writeSync_length(frame.as_mut_ptr().cast(), length) };
    }
}

fn spin_run_loop(limit: Duration) {
    let until = NSDate::dateWithTimeIntervalSinceNow(limit.as_secs_f64());
    unsafe { NSRunLoop::currentRunLoop().runMode_beforeDate(NSDefaultRunLoopMode, &until) };
}

fn connect(
    delegate: &Delegate,
    service: &Retained<IOBluetoothSDPUUID>,
    name_filter: Option<&str>,
) -> Option<Connection> {
    for device in connected_devices() {
        let name = unsafe { device.name() }.to_string();
        if name_filter.is_some_and(|filter| !name.contains(filter)) {
            continue;
        }

        let mut record = unsafe { device.getServiceRecordForUUID(Some(service)) };
        if record.is_none()
            && unsafe { device.deviceClassMajor() } == kBluetoothDeviceClassMajorAudio
        {
            query_service(delegate, &device, service);
            record = unsafe { device.getServiceRecordForUUID(Some(service)) };
        }
        let Some(record) = record else {
            continue;
        };
        let mut channel_id = 0;
        if unsafe { record.getRFCOMMChannelID(&mut channel_id) } != 0 {
            continue;
        }

        delegate.ivars().closed.store(false, Ordering::SeqCst);
        delegate.ivars().overflow.store(false, Ordering::SeqCst);
        if let Ok(mut incoming) = delegate.ivars().incoming.lock() {
            incoming.clear();
        }
        let mut channel = None;
        let status = unsafe {
            device.openRFCOMMChannelSync_withChannelID_delegate(
                Some(&mut channel),
                channel_id,
                Some(delegate),
            )
        };
        match channel {
            Some(channel) if status == 0 => {
                return Some(Connection {
                    name,
                    channel,
                    driver: SonyXm5Driver::default(),
                });
            }
            Some(channel) => unsafe {
                channel.closeChannel();
            },
            None => {}
        }
    }
    None
}

fn query_service(
    delegate: &Delegate,
    device: &IOBluetoothDevice,
    service: &Retained<IOBluetoothSDPUUID>,
) {
    delegate.ivars().sdp_done.store(false, Ordering::SeqCst);
    let object: Retained<AnyObject> = unsafe { Retained::cast_unchecked(service.clone()) };
    let services = objc2_foundation::NSArray::from_retained_slice(&[object]);
    unsafe { device.performSDPQuery_uuids(Some(delegate), Some(&services)) };
    let deadline = Instant::now() + SDP_TIMEOUT;
    while !delegate.ivars().sdp_done.load(Ordering::SeqCst) && Instant::now() < deadline {
        spin_run_loop(POLL);
    }
}

fn connected_devices() -> Vec<Retained<IOBluetoothDevice>> {
    let Some(paired) = (unsafe { IOBluetoothDevice::pairedDevices() }) else {
        return Vec::new();
    };
    paired
        .to_vec()
        .into_iter()
        .filter_map(|object| object.downcast::<IOBluetoothDevice>().ok())
        .filter(|device| unsafe { device.isConnected() })
        .collect()
}

fn sdp_uuid(uuid: &str) -> Option<Retained<IOBluetoothSDPUUID>> {
    let bytes = uuid_bytes(uuid)?;
    unsafe { IOBluetoothSDPUUID::uuidWithBytes_length(bytes.as_ptr().cast(), 16) }
}

fn uuid_bytes(uuid: &str) -> Option<[u8; 16]> {
    let digits: Vec<u8> = uuid.bytes().filter(|&byte| byte != b'-').collect();
    if digits.len() != 32 {
        return None;
    }
    let mut bytes = [0; 16];
    for (byte, pair) in bytes.iter_mut().zip(digits.chunks(2)) {
        *byte = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(bytes)
}

#[derive(Default)]
struct Ivars {
    incoming: Mutex<Vec<u8>>,
    closed: AtomicBool,
    overflow: AtomicBool,
    sdp_done: AtomicBool,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "IHeadphoneSonyChannelDelegate"]
    #[ivars = Ivars]
    struct Delegate;

    impl Delegate {
        #[unsafe(method(sdpQueryComplete:status:))]
        fn sdp_query_complete(&self, _device: *mut AnyObject, _status: i32) {
            self.ivars().sdp_done.store(true, Ordering::SeqCst);
        }
    }

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl IOBluetoothRFCOMMChannelDelegate for Delegate {
        #[unsafe(method(rfcommChannelData:data:length:))]
        unsafe fn rfcomm_channel_data(
            &self,
            _channel: Option<&IOBluetoothRFCOMMChannel>,
            data: *mut c_void,
            length: usize,
        ) {
            if data.is_null() || length == 0 {
                return;
            }
            let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) };
            if let Ok(mut incoming) = self.ivars().incoming.lock() {
                if incoming.len().saturating_add(bytes.len()) > MAX_INCOMING {
                    incoming.clear();
                    self.ivars().overflow.store(true, Ordering::SeqCst);
                } else {
                    incoming.extend_from_slice(bytes);
                }
            }
        }

        #[unsafe(method(rfcommChannelClosed:))]
        unsafe fn rfcomm_channel_closed(&self, _channel: Option<&IOBluetoothRFCOMMChannel>) {
            self.ivars().closed.store(true, Ordering::SeqCst);
        }
    }
);

impl Delegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(Ivars::default());
        unsafe { msg_send![super(this), init] }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sony_service_uuid() {
        let bytes = uuid_bytes(SERVICE_UUID).unwrap();
        assert_eq!(bytes[..4], [0x95, 0x6c, 0x7b, 0x26]);
        assert!(uuid_bytes("not-a-uuid").is_none());
    }
}
