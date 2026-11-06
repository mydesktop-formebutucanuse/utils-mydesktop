// SPDX-License-Identifier: GPL-3.0-or-later
//! The few pieces of Xlib, XInput2 and libc that mediakeyd needs.
//! Layouts follow <X11/Xlib.h>, <X11/extensions/XInput2.h> and <poll.h>.

use std::os::raw::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong, c_void};

pub enum Display {}
pub type Window = c_ulong;
pub type KeySym = c_ulong;
pub type KeyCode = c_uchar;
pub type Bool = c_int;
pub type Status = c_int;
pub type Time = c_ulong;

pub const SUCCESS: Status = 0;
pub const MAPPING_NOTIFY: c_int = 34;
pub const GENERIC_EVENT: c_int = 35;
pub const XI_ALL_MASTER_DEVICES: c_int = 1;
pub const XI_RAW_KEY_PRESS: c_int = 13;
pub const XI_RAW_KEY_RELEASE: c_int = 14;
pub const XKB_USE_CORE_KBD: c_uint = 0x0100;
pub const POLLIN: c_short = 1;

/// `XEvent` is a union padded to 24 longs.
#[repr(C)]
pub struct XEvent {
    pad: [c_long; 24],
}

impl XEvent {
    pub fn zeroed() -> Self {
        XEvent { pad: [0; 24] }
    }

    /// The `type` member shared by every event in the union.
    pub fn kind(&self) -> c_int {
        // SAFETY: every XEvent variant starts with `int type`.
        unsafe { *(self as *const XEvent as *const c_int) }
    }
}

#[repr(C)]
pub struct XGenericEventCookie {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub extension: c_int,
    pub evtype: c_int,
    pub cookie: c_uint,
    pub data: *mut c_void,
}

/// Leading part of `XIRawEvent`; the valuator fields after `flags` are not used.
#[repr(C)]
pub struct XIRawEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub extension: c_int,
    pub evtype: c_int,
    pub time: Time,
    pub deviceid: c_int,
    pub sourceid: c_int,
    pub detail: c_int,
    pub flags: c_int,
}

#[repr(C)]
pub struct XIEventMask {
    pub deviceid: c_int,
    pub mask_len: c_int,
    pub mask: *mut c_uchar,
}

#[repr(C)]
pub struct PollFd {
    pub fd: c_int,
    pub events: c_short,
    pub revents: c_short,
}

#[link(name = "X11")]
unsafe extern "C" {
    pub fn XOpenDisplay(name: *const c_char) -> *mut Display;
    pub fn XDefaultRootWindow(dpy: *mut Display) -> Window;
    pub fn XConnectionNumber(dpy: *mut Display) -> c_int;
    pub fn XPending(dpy: *mut Display) -> c_int;
    pub fn XNextEvent(dpy: *mut Display, ev: *mut XEvent) -> c_int;
    pub fn XGetEventData(dpy: *mut Display, cookie: *mut XGenericEventCookie) -> Bool;
    pub fn XFreeEventData(dpy: *mut Display, cookie: *mut XGenericEventCookie);
    pub fn XQueryExtension(
        dpy: *mut Display,
        name: *const c_char,
        major_opcode: *mut c_int,
        first_event: *mut c_int,
        first_error: *mut c_int,
    ) -> Bool;
    pub fn XStringToKeysym(name: *const c_char) -> KeySym;
    pub fn XKeysymToKeycode(dpy: *mut Display, keysym: KeySym) -> KeyCode;
    /// Takes an `XMappingEvent *`, which is a member of the `XEvent` union.
    pub fn XRefreshKeyboardMapping(ev: *mut XEvent) -> c_int;
    pub fn XQueryKeymap(dpy: *mut Display, keys: *mut c_char) -> c_int;
    pub fn XkbGetAutoRepeatRate(
        dpy: *mut Display,
        device_spec: c_uint,
        delay: *mut c_uint,
        interval: *mut c_uint,
    ) -> Bool;
}

#[link(name = "Xi")]
unsafe extern "C" {
    pub fn XIQueryVersion(dpy: *mut Display, major: *mut c_int, minor: *mut c_int) -> Status;
    pub fn XISelectEvents(
        dpy: *mut Display,
        win: Window,
        masks: *mut XIEventMask,
        num_masks: c_int,
    ) -> c_int;
}

unsafe extern "C" {
    pub fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
}
