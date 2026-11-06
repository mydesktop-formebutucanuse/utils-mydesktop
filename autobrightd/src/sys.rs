// SPDX-License-Identifier: GPL-3.0-or-later
//! sysfs files kept open for the life of the daemon, kernel uevents, and
//! poll().

use std::ffi::{c_int, c_short, c_ulong, c_void};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// An open sysfs attribute. sysfs regenerates the value on every read
/// from offset 0, so pread() on the same descriptor replaces
/// open/read/close.
pub struct Attr {
    pub path: PathBuf,
    file: File,
}

impl Attr {
    pub fn open(path: impl Into<PathBuf>, write: bool) -> io::Result<Attr> {
        let path = path.into();
        let file = OpenOptions::new().read(true).write(write).open(&path)?;
        Ok(Attr { path, file })
    }

    pub fn read_str<'a>(&self, buf: &'a mut [u8; 32]) -> io::Result<&'a str> {
        let n = self.file.read_at(buf, 0)?;
        std::str::from_utf8(&buf[..n])
            .map(str::trim)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
    }

    pub fn read_int(&self) -> io::Result<i64> {
        let mut buf = [0; 32];
        self.read_str(&mut buf)?
            .parse()
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))
    }

    pub fn read_u32(&self) -> io::Result<u32> {
        Ok(self.read_int()?.clamp(0, u32::MAX as i64) as u32)
    }

    pub fn write_u32(&self, value: u32) -> io::Result<()> {
        let text = format!("{value}\n");
        self.file.write_at(text.as_bytes(), 0).map(drop)
    }

    pub fn fd(&self) -> c_int {
        self.file.as_raw_fd()
    }
}

/// Reads a whole small sysfs file once (for max_brightness and names).
pub fn read_once(path: &Path) -> io::Result<String> {
    Ok(fs::read_to_string(path)?.trim().to_owned())
}

/// The IIO device whose name is `name`, e.g. aop-sensors-als.
pub fn find_iio(name: &str) -> Option<PathBuf> {
    fs::read_dir("/sys/bus/iio/devices").ok()?.flatten().map(|e| e.path()).find(|dir| {
        read_once(&dir.join("name")).is_ok_and(|n| n == name)
    })
}

/// The dpms attribute of the internal panel (cardN-eDP-M). External
/// monitors do not matter for the built-in keyboard.
pub fn find_edp_dpms() -> Option<PathBuf> {
    fs::read_dir("/sys/class/drm").ok()?.flatten().find_map(|e| {
        let name = e.file_name();
        let name = name.to_str()?;
        let (card, connector) = name.split_once('-')?;
        (card.starts_with("card") && connector.starts_with("eDP-"))
            .then(|| e.path().join("dpms"))
    })
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: c_short,
    revents: c_short,
}

#[repr(C)]
struct SockaddrNl {
    family: u16,
    pad: u16,
    pid: u32,
    groups: u32,
}

const POLLIN: c_short = 0x1;
const POLLPRI: c_short = 0x2;
const POLLERR: c_short = 0x8;
const AF_NETLINK: c_int = 16;
const SOCK_DGRAM: c_int = 2;
const SOCK_NONBLOCK: c_int = 0o4000;
const SOCK_CLOEXEC: c_int = 0o2000000;
const NETLINK_KOBJECT_UEVENT: c_int = 15;
const ENOBUFS: i32 = 105;

unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    fn socket(domain: c_int, ty: c_int, protocol: c_int) -> c_int;
    fn bind(fd: c_int, addr: *const SockaddrNl, len: u32) -> c_int;
    fn recv(fd: c_int, buf: *mut c_void, len: usize, flags: c_int) -> isize;
}

/// Kernel uevents (netlink group 1), which need no privileges to read.
pub struct Uevents {
    fd: OwnedFd,
    buf: Box<[u8]>,
}

impl Uevents {
    pub fn open() -> io::Result<Uevents> {
        let ty = SOCK_DGRAM | SOCK_NONBLOCK | SOCK_CLOEXEC;
        // SAFETY: plain system call; the descriptor is owned at once.
        let raw = unsafe { socket(AF_NETLINK, ty, NETLINK_KOBJECT_UEVENT) };
        if raw < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: raw is a new descriptor that nothing else owns.
        let fd = unsafe { OwnedFd::from_raw_fd(raw) };
        let addr = SockaddrNl { family: AF_NETLINK as u16, pad: 0, pid: 0, groups: 1 };
        let len = size_of::<SockaddrNl>() as u32;
        // SAFETY: addr is a valid sockaddr_nl of the given length.
        if unsafe { bind(fd.as_raw_fd(), &addr, len) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Uevents { fd, buf: vec![0; 8192].into_boxed_slice() })
    }

    pub fn fd(&self) -> c_int {
        self.fd.as_raw_fd()
    }

    /// Reads every queued event. Returns true if one was about a power
    /// supply whose name starts with `prefix`, or if events were lost.
    pub fn drain(&mut self, prefix: &str) -> bool {
        let mut hit = false;
        loop {
            let (ptr, len) = (self.buf.as_mut_ptr().cast(), self.buf.len());
            // SAFETY: buf is valid for len bytes; the socket is non-blocking.
            let n = unsafe { recv(self.fd.as_raw_fd(), ptr, len, 0) };
            if n < 0 {
                if io::Error::last_os_error().raw_os_error() == Some(ENOBUFS) {
                    hit = true;
                    continue;
                }
                return hit; // EAGAIN: nothing left
            }
            let (mut power, mut named) = (false, false);
            for field in self.buf[..n as usize].split(|&b| b == 0) {
                power |= field == b"SUBSYSTEM=power_supply";
                if let Some(name) = field.strip_prefix(b"POWER_SUPPLY_NAME=") {
                    named = name.starts_with(prefix.as_bytes());
                }
            }
            hit |= power && named;
        }
    }
}

pub struct Ready {
    /// sysfs_notify() fired; read the attribute again to re-arm it.
    pub notify: bool,
    pub uevent: bool,
}

/// Waits up to `timeout` for sysfs_notify() on `notify` (POLLPRI) or for
/// data on the uevent socket.
pub fn wait(notify: Option<c_int>, uevent: Option<c_int>, timeout: Duration) -> io::Result<Ready> {
    let mut fds = [
        PollFd { fd: notify.unwrap_or(-1), events: POLLPRI, revents: 0 },
        PollFd { fd: uevent.unwrap_or(-1), events: POLLIN, revents: 0 },
    ];
    // Round up so we do not wake just before a deadline and spin.
    let ms = timeout.as_micros().div_ceil(1000).min(c_int::MAX as u128) as c_int;
    // SAFETY: two valid PollFds; a negative fd is ignored by poll().
    let n = unsafe { poll(fds.as_mut_ptr(), fds.len() as c_ulong, ms) };
    if n < 0 {
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
        fds[0].revents = 0;
        fds[1].revents = 0;
    }
    Ok(Ready {
        notify: fds[0].revents & POLLPRI != 0,
        uevent: fds[1].revents & (POLLIN | POLLERR) != 0,
    })
}
