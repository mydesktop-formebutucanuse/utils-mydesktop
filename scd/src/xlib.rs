// SPDX-License-Identifier: GPL-3.0-or-later
//! The few pieces of Xlib and libc that scd needs.
//! Layouts follow <X11/Xlib.h>, <poll.h> and <signal.h>.

use std::os::raw::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong, c_void};

pub enum Display {}
pub type Window = c_ulong;
pub type Atom = c_ulong;
pub type Bool = c_int;
pub type Time = c_ulong;

pub const SUCCESS: c_int = 0;
pub const FALSE: Bool = 0;
pub const NONE: c_ulong = 0;
pub const XA_RESOURCE_MANAGER: Atom = 23;
pub const XA_STRING: Atom = 31;
pub const PROP_MODE_REPLACE: c_int = 0;
pub const PROPERTY_NEW_VALUE: c_int = 0;
pub const PROPERTY_CHANGE_MASK: c_long = 1 << 22;
pub const STRUCTURE_NOTIFY_MASK: c_long = 1 << 17;
pub const PROPERTY_NOTIFY: c_int = 28;
pub const SELECTION_CLEAR: c_int = 29;
pub const CLIENT_MESSAGE: c_int = 33;
pub const POLLIN: c_short = 1;
pub const EINTR: c_int = 4;
pub const SIGHUP: c_int = 1;
pub const SIGINT: c_int = 2;
pub const SIGTERM: c_int = 15;

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

    /// Views the event as one of the members of the union.
    ///
    /// # Safety
    /// `T` must be the member that `kind()` designates.
    pub unsafe fn as_ref<T>(&self) -> &T {
        unsafe { &*(self as *const XEvent as *const T) }
    }
}

#[repr(C)]
pub struct XPropertyEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub window: Window,
    pub atom: Atom,
    pub time: Time,
    pub state: c_int,
}

#[repr(C)]
pub struct XSelectionClearEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub window: Window,
    pub selection: Atom,
    pub time: Time,
}

#[repr(C)]
pub struct XClientMessageEvent {
    pub type_: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub window: Window,
    pub message_type: Atom,
    pub format: c_int,
    pub data: [c_long; 5],
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
    pub fn XCloseDisplay(dpy: *mut Display) -> c_int;
    pub fn XDefaultScreen(dpy: *mut Display) -> c_int;
    pub fn XRootWindow(dpy: *mut Display, screen: c_int) -> Window;
    pub fn XConnectionNumber(dpy: *mut Display) -> c_int;
    pub fn XPending(dpy: *mut Display) -> c_int;
    pub fn XNextEvent(dpy: *mut Display, ev: *mut XEvent) -> c_int;
    pub fn XSendEvent(dpy: *mut Display, win: Window, propagate: Bool, mask: c_long, ev: *mut XEvent) -> c_int;
    pub fn XFlush(dpy: *mut Display) -> c_int;
    pub fn XSync(dpy: *mut Display, discard: Bool) -> c_int;
    pub fn XInternAtom(dpy: *mut Display, name: *const c_char, only_if_exists: Bool) -> Atom;
    pub fn XCreateSimpleWindow(
        dpy: *mut Display,
        parent: Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        border: c_ulong,
        background: c_ulong,
    ) -> Window;
    pub fn XDestroyWindow(dpy: *mut Display, win: Window) -> c_int;
    pub fn XSelectInput(dpy: *mut Display, win: Window, mask: c_long) -> c_int;
    pub fn XGetSelectionOwner(dpy: *mut Display, selection: Atom) -> Window;
    pub fn XSetSelectionOwner(dpy: *mut Display, selection: Atom, owner: Window, time: Time) -> c_int;
    pub fn XGrabServer(dpy: *mut Display) -> c_int;
    pub fn XUngrabServer(dpy: *mut Display) -> c_int;
    pub fn XChangeProperty(
        dpy: *mut Display,
        win: Window,
        property: Atom,
        type_: Atom,
        format: c_int,
        mode: c_int,
        data: *const c_uchar,
        nelements: c_int,
    ) -> c_int;
    pub fn XDeleteProperty(dpy: *mut Display, win: Window, property: Atom) -> c_int;
    pub fn XGetWindowProperty(
        dpy: *mut Display,
        win: Window,
        property: Atom,
        long_offset: c_long,
        long_length: c_long,
        delete: Bool,
        req_type: Atom,
        actual_type: *mut Atom,
        actual_format: *mut c_int,
        nitems: *mut c_ulong,
        bytes_after: *mut c_ulong,
        prop: *mut *mut c_uchar,
    ) -> c_int;
    pub fn XFree(data: *mut c_void) -> c_int;
}

unsafe extern "C" {
    pub fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    pub fn pipe(fds: *mut c_int) -> c_int;
    pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    pub fn signal(signum: c_int, handler: extern "C" fn(c_int)) -> usize;
}
