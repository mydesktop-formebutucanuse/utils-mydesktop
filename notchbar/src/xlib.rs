// SPDX-License-Identifier: GPL-3.0-or-later
//! The few pieces of Xlib, Xft and libc that notchbar needs.
//! Layouts follow <X11/Xlib.h>, <X11/Xft/Xft.h>, <poll.h> and <time.h>.

use std::os::raw::{c_char, c_int, c_long, c_short, c_uchar, c_uint, c_ulong, c_ushort, c_void};

pub enum Display {}
pub enum Visual {}
pub enum Gc {}
pub enum XftDraw {}
pub enum XErrorEvent {}
pub type Window = c_ulong;
pub type Drawable = c_ulong;
pub type Pixmap = c_ulong;
pub type Colormap = c_ulong;
pub type Atom = c_ulong;
pub type Cursor = c_ulong;
pub type Bool = c_int;
pub type ErrorHandler = unsafe extern "C" fn(*mut Display, *mut XErrorEvent) -> c_int;

pub const FALSE: Bool = 0;
pub const SUCCESS: c_int = 0;
pub const XA_ATOM: Atom = 4;
pub const XA_CARDINAL: Atom = 6;
pub const XA_PIXMAP: Atom = 20;
pub const XA_STRING: Atom = 31;
pub const XA_WM_HINTS: Atom = 35;
pub const XA_WM_NAME: Atom = 39;
pub const XA_WM_NORMAL_HINTS: Atom = 40;
pub const XA_WM_SIZE_HINTS: Atom = 41;
pub const XA_WM_CLASS: Atom = 67;
pub const PROP_MODE_REPLACE: c_int = 0;
pub const EXPOSURE_MASK: c_long = 1 << 15;
pub const SUBSTRUCTURE_NOTIFY_MASK: c_long = 1 << 19;
pub const PROPERTY_CHANGE_MASK: c_long = 1 << 22;
pub const EXPOSE: c_int = 12;
pub const CONFIGURE_NOTIFY: c_int = 22;
pub const PROPERTY_NOTIFY: c_int = 28;
pub const INPUT_OUTPUT: c_uint = 1;
pub const CW_OVERRIDE_REDIRECT: c_ulong = 1 << 9;
pub const FILL_TILED: c_int = 1;
pub const Z_PIXMAP: c_int = 2;
pub const ALL_PLANES: c_ulong = !0;
pub const POLLIN: c_short = 1;
pub const LC_TIME: c_int = 2;

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

    /// `window` and `atom` of an `XPropertyEvent`: after `type`, `serial`,
    /// `send_event` and `display`, each one long (or padded to one).
    pub fn property(&self) -> (Window, Atom) {
        (self.pad[4] as Window, self.pad[5] as Atom)
    }

    /// `window` and `above` of an `XConfigureEvent`: `window` follows
    /// `event`; `x`, `y`, `width`, `height` and `border_width` are ints, so
    /// `above` is the tenth long.
    pub fn configure(&self) -> (Window, Window) {
        (self.pad[5] as Window, self.pad[9] as Window)
    }
}

#[repr(C)]
#[derive(Default)]
pub struct XSetWindowAttributes {
    pub background_pixmap: Pixmap,
    pub background_pixel: c_ulong,
    pub border_pixmap: Pixmap,
    pub border_pixel: c_ulong,
    pub bit_gravity: c_int,
    pub win_gravity: c_int,
    pub backing_store: c_int,
    pub backing_planes: c_ulong,
    pub backing_pixel: c_ulong,
    pub save_under: Bool,
    pub event_mask: c_long,
    pub do_not_propagate_mask: c_long,
    pub override_redirect: Bool,
    pub colormap: Colormap,
    pub cursor: Cursor,
}

/// The leading members of `XImage`, as far as `bits_per_pixel`. Only read
/// through the pointer XGetImage returns; never allocated here.
#[repr(C)]
pub struct XImage {
    pub width: c_int,
    pub height: c_int,
    pub xoffset: c_int,
    pub format: c_int,
    pub data: *mut c_uchar,
    pub byte_order: c_int,
    pub bitmap_unit: c_int,
    pub bitmap_bit_order: c_int,
    pub bitmap_pad: c_int,
    pub depth: c_int,
    pub bytes_per_line: c_int,
    pub bits_per_pixel: c_int,
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

#[repr(C)]
pub struct XftFont {
    pub ascent: c_int,
    pub descent: c_int,
    pub height: c_int,
    pub max_advance_width: c_int,
    pub charset: *mut c_void,
    pub pattern: *mut c_void,
}

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
    pub fn XCreateWindow(
        dpy: *mut Display,
        parent: Window,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        border_width: c_uint,
        depth: c_int,
        class: c_uint,
        visual: *mut Visual,
        valuemask: c_ulong,
        attributes: *mut XSetWindowAttributes,
    ) -> Window;
    pub fn XMapWindow(dpy: *mut Display, win: Window) -> c_int;
    pub fn XLowerWindow(dpy: *mut Display, win: Window) -> c_int;
    pub fn XSetWindowBackgroundPixmap(dpy: *mut Display, win: Window, pixmap: Pixmap) -> c_int;
    pub fn XClearWindow(dpy: *mut Display, win: Window) -> c_int;
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
    pub fn XSetErrorHandler(handler: Option<ErrorHandler>) -> Option<ErrorHandler>;
    pub fn XCreatePixmap(dpy: *mut Display, d: Drawable, width: c_uint, height: c_uint, depth: c_uint) -> Pixmap;
    pub fn XCreateGC(dpy: *mut Display, d: Drawable, mask: c_ulong, values: *mut c_void) -> *mut Gc;
    pub fn XFreeGC(dpy: *mut Display, gc: *mut Gc) -> c_int;
    pub fn XSetTile(dpy: *mut Display, gc: *mut Gc, tile: Pixmap) -> c_int;
    pub fn XSetTSOrigin(dpy: *mut Display, gc: *mut Gc, x: c_int, y: c_int) -> c_int;
    pub fn XSetFillStyle(dpy: *mut Display, gc: *mut Gc, style: c_int) -> c_int;
    pub fn XFillRectangle(dpy: *mut Display, d: Drawable, gc: *mut Gc, x: c_int, y: c_int, w: c_uint, h: c_uint) -> c_int;
    pub fn XGetImage(
        dpy: *mut Display,
        d: Drawable,
        x: c_int,
        y: c_int,
        width: c_uint,
        height: c_uint,
        plane_mask: c_ulong,
        format: c_int,
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
    pub fn XftDrawCreate(dpy: *mut Display, d: Drawable, visual: *mut Visual, cmap: Colormap) -> *mut XftDraw;
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
    pub fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
    pub fn tzset();
    pub fn time(t: *mut c_long) -> c_long;
    pub fn localtime_r(t: *const c_long, tm: *mut Tm) -> *mut Tm;
    pub fn strftime(s: *mut c_char, max: usize, format: *const c_char, tm: *const Tm) -> usize;
}
