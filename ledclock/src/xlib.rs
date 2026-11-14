// SPDX-License-Identifier: GPL-3.0-or-later
//! The few pieces of Xlib, Xft and libc that ledclock needs.
//! Layouts follow <X11/Xlib.h>, <X11/Xft/Xft.h>, <poll.h>, <time.h> and
//! <sys/timerfd.h>.

use std::os::raw::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong, c_ushort, c_void};

pub enum Display {}
pub enum Visual {}
pub enum Gc {}
pub enum XftDraw {}
pub type Window = c_ulong;
pub type Drawable = c_ulong;
pub type Pixmap = c_ulong;
pub type Colormap = c_ulong;
pub type Atom = c_ulong;
pub type Cursor = c_ulong;
pub type Bool = c_int;

pub const XA_ATOM: Atom = 4;
pub const XA_CARDINAL: Atom = 6;
pub const XA_STRING: Atom = 31;
pub const XA_WM_HINTS: Atom = 35;
pub const XA_WM_NAME: Atom = 39;
pub const XA_WM_NORMAL_HINTS: Atom = 40;
pub const XA_WM_SIZE_HINTS: Atom = 41;
pub const XA_WM_CLASS: Atom = 67;
pub const PROP_MODE_REPLACE: c_int = 0;
pub const BUTTON_PRESS_MASK: c_long = 1 << 2;
pub const BUTTON_RELEASE_MASK: c_long = 1 << 3;
pub const POINTER_MOTION_MASK: c_long = 1 << 6;
pub const EXPOSURE_MASK: c_long = 1 << 15;
pub const BUTTON_PRESS: c_int = 4;
pub const BUTTON_RELEASE: c_int = 5;
pub const MOTION_NOTIFY: c_int = 6;
pub const EXPOSE: c_int = 12;
/// Shapes of the cursor font (<X11/cursorfont.h>).
pub const XC_BOTTOM_LEFT_CORNER: c_uint = 12;
pub const XC_BOTTOM_RIGHT_CORNER: c_uint = 14;
pub const XC_FLEUR: c_uint = 52;
pub const XC_TOP_LEFT_CORNER: c_uint = 134;
pub const XC_TOP_RIGHT_CORNER: c_uint = 136;
pub const Z_PIXMAP: c_int = 2;
pub const LSB_FIRST: c_int = 0;
pub const MSB_FIRST: c_int = 1;
pub const POLLIN: c_short = 1;
pub const CLOCK_REALTIME: c_int = 0;
pub const TFD_CLOEXEC: c_int = 0o2000000;
pub const TFD_TIMER_ABSTIME: c_int = 1;
pub const TFD_TIMER_CANCEL_ON_SET: c_int = 2;

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

    /// The event as one of a button or of the pointer's motion. Only for
    /// an event whose `kind()` is one of those.
    pub fn pointer(&self) -> &XPointerEvent {
        // SAFETY: XButtonEvent and XMotionEvent both start with these
        // members, and the union is larger and aligned for them.
        unsafe { &*(self as *const XEvent as *const XPointerEvent) }
    }
}

/// The members `XButtonEvent` and `XMotionEvent` share, and the one after
/// them: the button of a button event.
#[repr(C)]
pub struct XPointerEvent {
    pub kind: c_int,
    pub serial: c_ulong,
    pub send_event: Bool,
    pub display: *mut Display,
    pub window: Window,
    pub root: Window,
    pub subwindow: Window,
    pub time: c_ulong,
    pub x: c_int,
    pub y: c_int,
    pub x_root: c_int,
    pub y_root: c_int,
    pub state: c_uint,
    pub button: c_uint,
}

/// The leading members of `XImage`, as far as the colour masks. Only used
/// through the pointer XCreateImage returns; never allocated here.
#[repr(C)]
pub struct XImage {
    pub width: c_int,
    pub height: c_int,
    pub xoffset: c_int,
    pub format: c_int,
    pub data: *mut c_char,
    pub byte_order: c_int,
    pub bitmap_unit: c_int,
    pub bitmap_bit_order: c_int,
    pub bitmap_pad: c_int,
    pub depth: c_int,
    pub bytes_per_line: c_int,
    pub bits_per_pixel: c_int,
    pub red_mask: c_ulong,
    pub green_mask: c_ulong,
    pub blue_mask: c_ulong,
}

#[repr(C)]
pub struct XRenderColor {
    pub red: c_ushort,
    pub green: c_ushort,
    pub blue: c_ushort,
    pub alpha: c_ushort,
}

#[repr(C)]
pub struct XftColor {
    pub pixel: c_ulong,
    pub color: XRenderColor,
}

pub enum XftFont {}

#[repr(C)]
#[derive(Default)]
pub struct XGlyphInfo {
    pub width: c_ushort,
    pub height: c_ushort,
    pub x: c_short,
    pub y: c_short,
    pub x_off: c_short,
    pub y_off: c_short,
}

#[repr(C)]
pub struct PollFd {
    pub fd: c_int,
    pub events: c_short,
    pub revents: c_short,
}

/// glibc's `struct tm`.
#[repr(C)]
pub struct Tm {
    pub fields: [c_int; 9],
    pub gmtoff: c_long,
    pub zone: *const c_char,
}

#[repr(C)]
#[derive(Default)]
pub struct Timespec {
    pub sec: c_long,
    pub nsec: c_long,
}

#[repr(C)]
#[derive(Default)]
pub struct Itimerspec {
    pub interval: Timespec,
    pub value: Timespec,
}

#[link(name = "X11")]
unsafe extern "C" {
    pub fn XOpenDisplay(name: *const c_char) -> *mut Display;
    pub fn XDefaultScreen(dpy: *mut Display) -> c_int;
    pub fn XRootWindow(dpy: *mut Display, screen: c_int) -> Window;
    pub fn XDisplayWidth(dpy: *mut Display, screen: c_int) -> c_int;
    pub fn XDisplayHeight(dpy: *mut Display, screen: c_int) -> c_int;
    pub fn XDefaultVisual(dpy: *mut Display, screen: c_int) -> *mut Visual;
    pub fn XDefaultColormap(dpy: *mut Display, screen: c_int) -> Colormap;
    pub fn XDefaultDepth(dpy: *mut Display, screen: c_int) -> c_int;
    pub fn XConnectionNumber(dpy: *mut Display) -> c_int;
    pub fn XPending(dpy: *mut Display) -> c_int;
    pub fn XNextEvent(dpy: *mut Display, ev: *mut XEvent) -> c_int;
    pub fn XFlush(dpy: *mut Display) -> c_int;
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
    pub fn XMapWindow(dpy: *mut Display, win: Window) -> c_int;
    pub fn XMoveResizeWindow(dpy: *mut Display, win: Window, x: c_int, y: c_int, width: c_uint, height: c_uint)
    -> c_int;
    pub fn XSetWindowBackgroundPixmap(dpy: *mut Display, win: Window, pixmap: Pixmap) -> c_int;
    pub fn XCreateFontCursor(dpy: *mut Display, shape: c_uint) -> Cursor;
    pub fn XDefineCursor(dpy: *mut Display, win: Window, cursor: Cursor) -> c_int;
    pub fn XUndefineCursor(dpy: *mut Display, win: Window) -> c_int;
    pub fn XSelectInput(dpy: *mut Display, win: Window, mask: c_long) -> c_int;
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
    pub fn XCreatePixmap(dpy: *mut Display, d: Drawable, width: c_uint, height: c_uint, depth: c_uint) -> Pixmap;
    pub fn XFreePixmap(dpy: *mut Display, pixmap: Pixmap) -> c_int;
    pub fn XCreateGC(dpy: *mut Display, d: Drawable, mask: c_ulong, values: *mut c_void) -> *mut Gc;
    pub fn XSetGraphicsExposures(dpy: *mut Display, gc: *mut Gc, graphics_exposures: Bool) -> c_int;
    pub fn XCreateImage(
        dpy: *mut Display,
        visual: *mut Visual,
        depth: c_uint,
        format: c_int,
        offset: c_int,
        data: *mut c_char,
        width: c_uint,
        height: c_uint,
        bitmap_pad: c_int,
        bytes_per_line: c_int,
    ) -> *mut XImage;
    pub fn XPutImage(
        dpy: *mut Display,
        d: Drawable,
        gc: *mut Gc,
        image: *mut XImage,
        src_x: c_int,
        src_y: c_int,
        dst_x: c_int,
        dst_y: c_int,
        width: c_uint,
        height: c_uint,
    ) -> c_int;
    pub fn XDestroyImage(image: *mut XImage) -> c_int;
    pub fn XCopyArea(
        dpy: *mut Display,
        src: Drawable,
        dst: Drawable,
        gc: *mut Gc,
        src_x: c_int,
        src_y: c_int,
        width: c_uint,
        height: c_uint,
        dst_x: c_int,
        dst_y: c_int,
    ) -> c_int;
}

#[link(name = "Xft")]
unsafe extern "C" {
    pub fn XftFontOpenName(dpy: *mut Display, screen: c_int, name: *const c_char) -> *mut XftFont;
    pub fn XftFontClose(dpy: *mut Display, font: *mut XftFont);
    pub fn XftDrawCreate(dpy: *mut Display, d: Drawable, visual: *mut Visual, cmap: Colormap) -> *mut XftDraw;
    pub fn XftDrawChange(draw: *mut XftDraw, d: Drawable);
    pub fn XftColorAllocValue(
        dpy: *mut Display,
        visual: *mut Visual,
        cmap: Colormap,
        color: *const XRenderColor,
        result: *mut XftColor,
    ) -> Bool;
    pub fn XftDrawStringUtf8(
        draw: *mut XftDraw,
        color: *const XftColor,
        font: *mut XftFont,
        x: c_int,
        y: c_int,
        s: *const c_uchar,
        len: c_int,
    );
    pub fn XftTextExtentsUtf8(
        dpy: *mut Display,
        font: *mut XftFont,
        s: *const c_uchar,
        len: c_int,
        extents: *mut XGlyphInfo,
    );
}

unsafe extern "C" {
    pub fn poll(fds: *mut PollFd, nfds: c_ulong, timeout: c_int) -> c_int;
    pub fn read(fd: c_int, buf: *mut c_void, count: usize) -> isize;
    pub fn tzset();
    pub fn time(t: *mut c_long) -> c_long;
    pub fn localtime_r(t: *const c_long, tm: *mut Tm) -> *mut Tm;
    pub fn timerfd_create(clock: c_int, flags: c_int) -> c_int;
    pub fn timerfd_settime(fd: c_int, flags: c_int, new: *const Itimerspec, old: *mut Itimerspec) -> c_int;
}
