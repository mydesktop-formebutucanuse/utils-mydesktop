// SPDX-License-Identifier: GPL-3.0-or-later
//! sysfs files kept open for the life of the daemon, and poll().

use std::ffi::{c_int, c_short, c_ulong};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
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

const POLLPRI: c_short = 0x2;

unsafe extern "C" {
    fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
}

/// Waits up to `timeout` for sysfs_notify() on `fd` (POLLPRI). Returns
/// true if it fired. The caller must read the attribute again to re-arm it.
pub fn wait_notify(fd: Option<c_int>, timeout: Duration) -> io::Result<bool> {
    let mut pfd = PollFd { fd: fd.unwrap_or(-1), events: POLLPRI, revents: 0 };
    // Round up so we do not wake just before a deadline and spin.
    let ms = timeout.as_micros().div_ceil(1000).min(c_int::MAX as u128) as c_int;
    // SAFETY: one valid PollFd; a negative fd is ignored by poll().
    let n = unsafe { poll(&mut pfd, 1, ms) };
    match n {
        -1 => {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted { Ok(false) } else { Err(e) }
        }
        0 => Ok(false),
        _ => Ok(pfd.revents & POLLPRI != 0),
    }
}
