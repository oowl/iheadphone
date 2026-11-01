use std::ffi::c_void;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::raw::c_long;
use std::time::Duration;

use bluer::Address;

const AF_BLUETOOTH: i32 = 31;
const SOCK_SEQPACKET: i32 = 5;
const SOCK_CLOEXEC: i32 = 0o2000000;
const SOCK_NONBLOCK: i32 = 0o4000;
const BTPROTO_L2CAP: i32 = 0;
const SOL_SOCKET: i32 = 1;
const SO_ERROR: i32 = 4;
const SO_SNDTIMEO: i32 = 21;
const SO_RCVTIMEO: i32 = 20;
const POLLOUT: i16 = 0x004;
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
const EINPROGRESS: i32 = 115;

#[repr(C)]
struct SockAddrL2 {
    family: u16,
    psm: u16,
    address: [u8; 6],
    cid: u16,
    address_type: u8,
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

#[repr(C)]
struct TimeVal {
    seconds: c_long,
    micros: c_long,
}

unsafe extern "C" {
    fn socket(domain: i32, kind: i32, protocol: i32) -> i32;
    fn connect(fd: i32, address: *const c_void, length: u32) -> i32;
    fn poll(fds: *mut PollFd, count: u64, timeout_ms: i32) -> i32;
    fn getsockopt(fd: i32, level: i32, name: i32, value: *mut c_void, length: *mut u32) -> i32;
    fn setsockopt(fd: i32, level: i32, name: i32, value: *const c_void, length: u32) -> i32;
    fn fcntl(fd: i32, command: i32, ...) -> i32;
    fn read(fd: i32, buffer: *mut c_void, count: usize) -> isize;
    fn write(fd: i32, buffer: *const c_void, count: usize) -> isize;
    fn shutdown(fd: i32, how: i32) -> i32;
}

pub struct Stream {
    fd: OwnedFd,
}

impl Stream {
    pub fn connect(address: Address, psm: u16, timeout: Duration) -> io::Result<Self> {
        let raw = unsafe {
            socket(
                AF_BLUETOOTH,
                SOCK_SEQPACKET | SOCK_CLOEXEC | SOCK_NONBLOCK,
                BTPROTO_L2CAP,
            )
        };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let mut bytes = address.0;
        bytes.reverse();
        let remote = SockAddrL2 {
            family: AF_BLUETOOTH as u16,
            psm: psm.to_le(),
            address: bytes,
            cid: 0,
            address_type: 0,
        };
        let result = unsafe {
            connect(
                fd.as_raw_fd(),
                (&raw const remote).cast(),
                size_of::<SockAddrL2>() as u32,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(EINPROGRESS) {
                return Err(error);
            }
            wait_until_connected(fd.as_raw_fd(), timeout)?;
        }
        set_blocking(fd.as_raw_fd())?;
        set_timeout(fd.as_raw_fd(), SO_SNDTIMEO, timeout)?;
        set_timeout(fd.as_raw_fd(), SO_RCVTIMEO, Duration::from_millis(250))?;
        Ok(Self { fd })
    }

    pub fn send(&self, packet: &[u8]) -> io::Result<()> {
        let count = unsafe { write(self.fd.as_raw_fd(), packet.as_ptr().cast(), packet.len()) };
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        if count as usize != packet.len() {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "partial L2CAP packet",
            ));
        }
        Ok(())
    }

    pub fn receive(&self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = unsafe {
            read(
                self.fd.as_raw_fd(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if count < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count as usize)
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        unsafe { shutdown(self.fd.as_raw_fd(), 2) };
    }
}

fn wait_until_connected(fd: i32, timeout: Duration) -> io::Result<()> {
    let mut poll_fd = PollFd {
        fd,
        events: POLLOUT,
        revents: 0,
    };
    let timeout_ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    let ready = unsafe { poll(&raw mut poll_fd, 1, timeout_ms) };
    if ready == 0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "AirPods L2CAP connect timed out",
        ));
    }
    if ready < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut socket_error = 0i32;
    let mut length = size_of::<i32>() as u32;
    if unsafe {
        getsockopt(
            fd,
            SOL_SOCKET,
            SO_ERROR,
            (&raw mut socket_error).cast(),
            &raw mut length,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    if socket_error != 0 {
        return Err(io::Error::from_raw_os_error(socket_error));
    }
    Ok(())
}

fn set_blocking(fd: i32) -> io::Result<()> {
    let flags = unsafe { fcntl(fd, F_GETFL) };
    if flags < 0 || unsafe { fcntl(fd, F_SETFL, flags & !SOCK_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn set_timeout(fd: i32, option: i32, timeout: Duration) -> io::Result<()> {
    let value = TimeVal {
        seconds: timeout.as_secs().min(c_long::MAX as u64) as c_long,
        micros: c_long::from(timeout.subsec_micros()),
    };
    if unsafe {
        setsockopt(
            fd,
            SOL_SOCKET,
            option,
            (&raw const value).cast(),
            size_of::<TimeVal>() as u32,
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
