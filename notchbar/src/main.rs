// SPDX-License-Identifier: GPL-3.0-or-later
//! notchbar: the clock and the battery beside the notch, as macOS shows them.
//!
//! With `appledrm.show_notch=1` the Asahi kernel gives X the whole panel,
//! the strip on both sides of the camera notch included. notchbar fills that
//! strip with a dock window, black at the top edge and fading into the
//! desktop background towards the bottom, so that the notch disappears into
//! it. The fade runs on below the strip, over the desktop background only,
//! and ends without an edge. It reserves the strip with a strut, so that
//! maximised windows stop
//! below it, and with `_WINDOWMAKER_FULLSCREEN_STRUT`, so that Window Maker
//! keeps fullscreen windows below it as well. On its right side it shows the
//! battery level and the time, like the macOS menu bar:
//! `80%  Tue Nov 10  2:32 PM`.
//!
//! The strip's height is the notch's: the panels of the notched MacBooks are
//! 16:10 below the notch, so it is the screen height less 10/16 of its width
//! (74 pixels on the 14" and 16" MacBook Pro).

mod xlib;

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs, process, ptr};

use xlib::*;

const USAGE: &str = "usage: notchbar [-H HEIGHT] [-g FADE] [-f TIME_FORMAT] [-F FONT] [-P]";
/// strftime(3) format of the clock, the one of the macOS menu bar.
const DEFAULT_FORMAT: &str = "%a %b %-d  %-I:%M %p";
/// Fontconfig pattern of the text; the pixel size follows the bar height.
const DEFAULT_FONT: &str = "Noto Sans CJK KR:medium";
/// Height of the notch the sizes below are drawn for (MacBook Pro, 2x).
const BASE_HEIGHT: f32 = 74.0;
/// How often the battery is read, in milliseconds.
const BATTERY_PERIOD_MS: u64 = 5000;
const POWER_SUPPLY: &str = "/sys/class/power_supply";

fn die(msg: &str) -> ! {
    eprintln!("notchbar: {msg}");
    process::exit(1)
}

struct Options {
    height: Option<u32>,
    /// How far the fade runs on below the strip, in pixels.
    fade: Option<u32>,
    format: String,
    font: String,
    percent: bool,
}

fn parse_args() -> Options {
    let mut o = Options { height: None, fade: None, format: DEFAULT_FORMAT.into(), font: DEFAULT_FONT.into(), percent: true };
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_else(|| die(USAGE));
        match a.as_str() {
            "-H" => o.height = Some(value().parse().ok().filter(|&h| h > 0).unwrap_or_else(|| die("bad height"))),
            "-g" => o.fade = Some(value().parse().unwrap_or_else(|_| die("bad fade length"))),
            "-f" => o.format = value(),
            "-F" => o.font = value(),
            "-P" => o.percent = false,
            "-h" | "--help" => {
                println!("{USAGE}");
                process::exit(0)
            }
            _ => die(USAGE),
        }
    }
    o
}

/// The notch's height on a screen of this size, 0 when the kernel hides it.
fn notch_height(width: i32, height: i32) -> i32 {
    let h = height - width * 10 / 16;
    if (1..=200).contains(&h) { h } else { 0 }
}

fn read_trimmed(path: &str) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string())
}

/// The level of the system battery, in percent.
fn read_battery() -> Option<u32> {
    for entry in fs::read_dir(POWER_SUPPLY).ok()?.flatten() {
        let dir = entry.path().to_string_lossy().into_owned();
        // scope Device is a mouse or a keyboard.
        if read_trimmed(&format!("{dir}/type")).as_deref() == Some("Battery")
            && read_trimmed(&format!("{dir}/scope")).as_deref() != Some("Device")
        {
            return read_trimmed(&format!("{dir}/capacity"))?.parse::<u32>().ok().map(|p| p.min(100));
        }
    }
    None
}

fn clock_text(format: &CString) -> String {
    let mut buf = [0u8; 256];
    // SAFETY: plain libc calls on local buffers.
    let n = unsafe {
        tzset();
        let t = time(ptr::null_mut());
        let mut tm = std::mem::zeroed::<Tm>();
        localtime_r(&t, &mut tm);
        strftime(buf.as_mut_ptr() as *mut c_char, buf.len(), format.as_ptr(), &tm)
    };
    String::from_utf8_lossy(&buf[..n]).into_owned()
}

/// How much of the desktop background shows through screen row `y` of a
/// fade `len` rows long, in 1/65536: none at the top edge of the screen, all
/// of it from the fade's last row on. (3t - t³)/2: it starts lightening
/// right at the top edge, and meets the background flat, so its end does
/// not show as an edge.
fn see_through(y: i32, len: i32) -> u32 {
    if len <= 1 {
        return 0;
    }
    let t = (y as f32 / (len - 1) as f32).clamp(0.0, 1.0);
    (65536.0 * t * (3.0 - t * t) / 2.0).round() as u32
}

// ---- X ----

/// Ignores X errors instead of exiting: a root background pixmap can be
/// freed by the program that set it between notchbar reading its id and
/// using it, and the strip then just stays black.
unsafe extern "C" fn ignore_error(_: *mut Display, _: *mut XErrorEvent) -> c_int {
    0
}

struct Bar {
    dpy: *mut Display,
    root: Window,
    win: Window,
    /// The strip's background: the desktop background under it, shaded.
    back: Pixmap,
    /// What is shown: `back` with the text on it.
    pixmap: Pixmap,
    /// Where the fade runs on below the strip, 0 when it does not: an
    /// override-redirect window under every other one, so that it is only
    /// seen where the desktop is. It selects no input, so that clicks on it
    /// reach the root window, as the window manager's root menu wants.
    fade: Window,
    /// The fade's background, which the server paints the window with.
    fade_pixmap: Pixmap,
    fade_height: i32,
    gc: *mut Gc,
    draw: *mut XftDraw,
    font: *mut XftFont,
    fg: XftColor,
    /// `_XROOTPMAP_ID` and `ESETROOT_PMAP_ID`, the root background pixmap.
    root_pmap: [Atom; 2],
    width: i32,
    height: i32,
    scale: f32,
    percent: bool,
}

fn atom(dpy: *mut Display, name: &str) -> Atom {
    let c = CString::new(name).unwrap();
    // SAFETY: valid display and NUL-terminated name.
    unsafe { XInternAtom(dpy, c.as_ptr(), FALSE) }
}

fn set_longs(dpy: *mut Display, win: Window, prop: Atom, ty: Atom, data: &[c_long]) {
    // SAFETY: format 32 properties are passed to Xlib as an array of longs.
    unsafe {
        XChangeProperty(dpy, win, prop, ty, 32, PROP_MODE_REPLACE, data.as_ptr() as *const c_uchar, data.len() as c_int)
    };
}

fn set_bytes(dpy: *mut Display, win: Window, prop: Atom, ty: Atom, data: &[u8]) {
    // SAFETY: format 8 data of the given length.
    unsafe { XChangeProperty(dpy, win, prop, ty, 8, PROP_MODE_REPLACE, data.as_ptr(), data.len() as c_int) };
}

fn color(dpy: *mut Display, visual: *mut Visual, cmap: Colormap, rgb: u32) -> XftColor {
    let v = |shift: u32| (((rgb >> shift) & 0xff) * 0x101) as u16;
    let rc = XRenderColor { red: v(16), green: v(8), blue: v(0), alpha: 0xffff };
    let mut c = XftColor { pixel: 0, color: XRenderColor { red: 0, green: 0, blue: 0, alpha: 0 } };
    // SAFETY: valid display, visual and colormap.
    if unsafe { XftColorAllocValue(dpy, visual, cmap, &rc, &mut c) } == 0 {
        die("cannot allocate a color");
    }
    c
}

impl Bar {
    fn open(opts: &Options) -> Bar {
        // SAFETY: Xlib calls on the display opened here; every pointer is
        // checked before it is used.
        unsafe {
            let dpy = XOpenDisplay(ptr::null());
            if dpy.is_null() {
                die("cannot open the display");
            }
            let screen = XDefaultScreen(dpy);
            let root = XRootWindow(dpy, screen);
            let width = XDisplayWidth(dpy, screen);
            let height = match opts.height {
                Some(h) => h as i32,
                None => match notch_height(width, XDisplayHeight(dpy, screen)) {
                    0 => die(
                        "the screen has no notch strip: boot with appledrm.show_notch=1, or give its height with -H",
                    ),
                    h => h,
                },
            };
            let scale = height as f32 / BASE_HEIGHT;
            let visual = XDefaultVisual(dpy, screen);
            let cmap = XDefaultColormap(dpy, screen);
            let depth = XDefaultDepth(dpy, screen);

            let win = XCreateSimpleWindow(dpy, root, 0, 0, width as c_uint, height as c_uint, 0, 0, 0);
            let fade_height = match opts.fade {
                Some(g) => (g as i32).min(XDisplayHeight(dpy, screen) - height).max(0),
                None => (height * 3 + 2) / 4,
            };
            let (fade, fade_pixmap) = if fade_height > 0 {
                let mut attrs = XSetWindowAttributes { override_redirect: 1, ..Default::default() };
                let fade = XCreateWindow(
                    dpy,
                    root,
                    0,
                    height,
                    width as c_uint,
                    fade_height as c_uint,
                    0,
                    0, // depth, visual: CopyFromParent
                    INPUT_OUTPUT,
                    ptr::null_mut(),
                    CW_OVERRIDE_REDIRECT,
                    &mut attrs,
                );
                set_bytes(dpy, fade, XA_WM_NAME, XA_STRING, b"notchbar fade");
                set_bytes(dpy, fade, XA_WM_CLASS, XA_STRING, b"notchbar\0Notchbar\0");
                (fade, XCreatePixmap(dpy, fade, width as c_uint, fade_height as c_uint, depth as c_uint))
            } else {
                (0, 0)
            };
            set_bytes(dpy, win, XA_WM_NAME, XA_STRING, b"notchbar");
            set_bytes(dpy, win, XA_WM_CLASS, XA_STRING, b"notchbar\0Notchbar\0");
            set_longs(dpy, win, atom(dpy, "_NET_WM_WINDOW_TYPE"), XA_ATOM, &[atom(dpy, "_NET_WM_WINDOW_TYPE_DOCK") as c_long]);
            let state = [atom(dpy, "_NET_WM_STATE_STICKY"), atom(dpy, "_NET_WM_STATE_ABOVE"), atom(dpy, "_NET_WM_STATE_SKIP_TASKBAR")];
            set_longs(dpy, win, atom(dpy, "_NET_WM_STATE"), XA_ATOM, &state.map(|a| a as c_long));
            set_longs(dpy, win, atom(dpy, "_NET_WM_DESKTOP"), XA_CARDINAL, &[0xffff_ffff]);
            let h = height as c_long;
            set_longs(dpy, win, atom(dpy, "_NET_WM_STRUT"), XA_CARDINAL, &[0, 0, h, 0]);
            // Window Maker: fullscreen windows stay out of the strip too.
            set_longs(dpy, win, atom(dpy, "_WINDOWMAKER_FULLSCREEN_STRUT"), XA_CARDINAL, &[0, 0, h, 0]);
            set_longs(
                dpy,
                win,
                atom(dpy, "_NET_WM_STRUT_PARTIAL"),
                XA_CARDINAL,
                &[0, 0, h, 0, 0, 0, 0, 0, 0, width as c_long - 1, 0, 0],
            );
            // No decorations (flags = MWM_HINTS_DECORATIONS, decorations = 0).
            let motif = atom(dpy, "_MOTIF_WM_HINTS");
            set_longs(dpy, win, motif, motif, &[2, 0, 0, 0, 0]);
            // Never focused (flags = InputHint, input = False).
            set_longs(dpy, win, XA_WM_HINTS, XA_WM_HINTS, &[1, 0, 0, 0, 0, 0, 0, 0, 0]);
            // Placed by us at 0,0 and of a fixed size:
            // USPosition | PPosition | PMinSize | PMaxSize.
            let (w, hh) = (width as c_long, h);
            let mut size = [0 as c_long; 18];
            size[..9].copy_from_slice(&[1 | 4 | 16 | 32, 0, 0, w, hh, w, hh, w, hh]);
            set_longs(dpy, win, XA_WM_NORMAL_HINTS, XA_WM_SIZE_HINTS, &size);

            XSetErrorHandler(Some(ignore_error));
            XSelectInput(dpy, win, EXPOSURE_MASK);
            // A new desktop background is announced on the root window, and
            // a window put under all the others is seen there too.
            XSelectInput(dpy, root, PROPERTY_CHANGE_MASK | if fade != 0 { SUBSTRUCTURE_NOTIFY_MASK } else { 0 });
            let back = XCreatePixmap(dpy, win, width as c_uint, height as c_uint, depth as c_uint);
            let pixmap = XCreatePixmap(dpy, win, width as c_uint, height as c_uint, depth as c_uint);
            // The default GC draws in pixel 0, black on a TrueColor screen.
            let gc = XCreateGC(dpy, pixmap, 0, ptr::null_mut());
            let draw = XftDrawCreate(dpy, pixmap, visual, cmap);
            let pattern = if opts.font.contains("size=") {
                opts.font.clone()
            } else {
                format!("{}:pixelsize={}", opts.font, (26.0 * scale).round())
            };
            let font = XftFontOpenName(dpy, screen, CString::new(pattern.clone()).unwrap().as_ptr());
            if font.is_null() || draw.is_null() {
                die(&format!("cannot open the font `{pattern}`"));
            }
            let bar = Bar {
                dpy,
                root,
                win,
                back,
                pixmap,
                fade,
                fade_pixmap,
                fade_height,
                gc,
                draw,
                font,
                fg: color(dpy, visual, cmap, 0xffffff),
                root_pmap: [atom(dpy, "_XROOTPMAP_ID"), atom(dpy, "ESETROOT_PMAP_ID")],
                width,
                height,
                scale,
                percent: opts.percent,
            };
            bar.shade_background();
            XMapWindow(dpy, win);
            if fade != 0 {
                XLowerWindow(dpy, fade);
                XMapWindow(dpy, fade);
            }
            XFlush(dpy);
            bar
        }
    }

    /// The root window's background pixmap, as desktop background setters
    /// (wmsetbg, feh, Esetroot) announce it; 0 when none is.
    fn root_pixmap(&self) -> Pixmap {
        for &prop in &self.root_pmap {
            let (mut ty, mut format, mut n, mut after) = (0, 0, 0, 0);
            let mut data = ptr::null_mut();
            // SAFETY: valid display and window; the reply is freed below.
            let pixmap = unsafe {
                if XGetWindowProperty(
                    self.dpy, self.root, prop, 0, 1, FALSE, XA_PIXMAP, &mut ty, &mut format, &mut n, &mut after,
                    &mut data,
                ) != SUCCESS
                    || data.is_null()
                {
                    continue;
                }
                // Format 32 data comes back as an array of longs.
                let p = if ty == XA_PIXMAP && format == 32 && n == 1 { *(data as *const Pixmap) } else { 0 };
                XFree(data as *mut c_void);
                p
            };
            if pixmap != 0 {
                return pixmap;
            }
        }
        0
    }

    /// Draws the strip's background and the fade from the desktop background.
    fn shade_background(&self) {
        let root_pixmap = self.root_pixmap();
        self.shade(root_pixmap, self.back, 0, self.height);
        if self.fade != 0 {
            self.shade(root_pixmap, self.fade_pixmap, self.height, self.fade_height);
            // SAFETY: our own window and pixmap. Set again, so that the
            // server takes the new contents, and painted from it.
            unsafe {
                XSetWindowBackgroundPixmap(self.dpy, self.fade, self.fade_pixmap);
                XClearWindow(self.dpy, self.fade);
            }
        }
    }

    /// Fills `dst`, `rows` rows high, with the desktop background of the
    /// screen rows from `top` on, darkened to black towards the top edge of
    /// the screen. Without a background, or on a screen that is not 32 bits
    /// per pixel, it is plain black.
    fn shade(&self, root_pixmap: Pixmap, dst: Pixmap, top: i32, rows: i32) {
        let (w, h) = (self.width as c_uint, rows as c_uint);
        // SAFETY: Xlib calls on our own pixmaps and GCs; the image is
        // checked before use and its rows are bytes_per_line apart.
        unsafe {
            XFillRectangle(self.dpy, dst, self.gc, 0, 0, w, h);
            if root_pixmap == 0 {
                return;
            }
            // Tiled from the screen's origin, like the root window does, so
            // a background smaller than the screen comes out right too.
            let tile_gc = XCreateGC(self.dpy, dst, 0, ptr::null_mut());
            XSetTile(self.dpy, tile_gc, root_pixmap);
            XSetFillStyle(self.dpy, tile_gc, FILL_TILED);
            XSetTSOrigin(self.dpy, tile_gc, 0, -top);
            XFillRectangle(self.dpy, dst, tile_gc, 0, 0, w, h);
            XFreeGC(self.dpy, tile_gc);

            let image = XGetImage(self.dpy, dst, 0, 0, w, h, ALL_PLANES, Z_PIXMAP);
            if image.is_null() {
                XFillRectangle(self.dpy, dst, self.gc, 0, 0, w, h);
                return;
            }
            let im = &*image;
            if im.bits_per_pixel == 32 {
                let len = self.height + self.fade_height;
                for y in 0..rows {
                    let k = see_through(top + y, len);
                    let row = std::slice::from_raw_parts_mut(
                        im.data.add(y as usize * im.bytes_per_line as usize),
                        self.width as usize * 4,
                    );
                    // Darkening scales every channel alike, whatever their order.
                    for b in row {
                        *b = ((*b as u32 * k + 0x8000) >> 16) as u8;
                    }
                }
                XPutImage(self.dpy, dst, self.gc, image, 0, 0, 0, 0, w, h);
            } else {
                XFillRectangle(self.dpy, dst, self.gc, 0, 0, w, h);
            }
            XDestroyImage(image);
        }
    }

    fn text_width(&self, s: &str) -> i32 {
        let mut g = XGlyphInfo::default();
        // SAFETY: valid font and string.
        unsafe { XftTextExtentsUtf8(self.dpy, self.font, s.as_ptr(), s.len() as c_int, &mut g) };
        g.x_off as i32
    }

    /// The baseline that centres the digits in the bar.
    fn baseline(&self) -> i32 {
        let mut g = XGlyphInfo::default();
        // SAFETY: valid font and string.
        unsafe { XftTextExtentsUtf8(self.dpy, self.font, b"0".as_ptr(), 1, &mut g) };
        (self.height - g.height as i32) / 2 + g.y as i32
    }

    fn text(&self, s: &str, x: i32) {
        // SAFETY: valid draw, color, font and string.
        unsafe { XftDrawStringUtf8(self.draw, &self.fg, self.font, x, self.baseline(), s.as_ptr(), s.len() as c_int) };
    }

    /// Paints the bar into the pixmap, from the right edge leftwards.
    fn paint(&self, clock: &str, battery: Option<u32>) {
        let s = |v: f32| (v * self.scale).round() as i32;
        // SAFETY: valid display, pixmaps and GC.
        unsafe {
            XCopyArea(self.dpy, self.back, self.pixmap, self.gc, 0, 0, self.width as c_uint, self.height as c_uint, 0, 0)
        };
        let mut right = self.width - s(30.0);
        let w = self.text_width(clock);
        self.text(clock, right - w);
        right -= w + s(28.0);

        if let Some(p) = battery.filter(|_| self.percent) {
            let p = format!("{p}%");
            self.text(&p, right - self.text_width(&p));
        }
        self.show();
    }

    fn show(&self) {
        // SAFETY: valid display, pixmap, window and GC.
        unsafe {
            XCopyArea(self.dpy, self.pixmap, self.win, self.gc, 0, 0, self.width as c_uint, self.height as c_uint, 0, 0);
            XFlush(self.dpy);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn main() {
    let opts = parse_args();
    // SAFETY: called before any other thread exists.
    unsafe { setlocale(LC_TIME, c"".as_ptr()) };
    let format = CString::new(opts.format.clone()).unwrap_or_else(|_| die("bad time format"));
    let bar = Bar::open(&opts);

    let mut shown: Option<(String, Option<u32>)> = None;
    let mut next_battery = 0;
    let mut battery = None;
    let mut ev = XEvent::zeroed();
    loop {
        let now = now_ms();
        if now >= next_battery {
            battery = read_battery();
            next_battery = now + BATTERY_PERIOD_MS;
        }
        let state = (clock_text(&format), battery);
        if shown.as_ref() != Some(&state) {
            bar.paint(&state.0, state.1);
            shown = Some(state);
        }

        // Wake at the next minute or the next battery reading.
        let now = now_ms();
        let next_minute = now - now % 60_000 + 60_000 + 5;
        let timeout = next_minute.min(next_battery).saturating_sub(now).max(1);
        let mut pfd = PollFd { fd: unsafe { XConnectionNumber(bar.dpy) }, events: POLLIN, revents: 0 };
        // SAFETY: XPending also reads what poll() saw; XNextEvent only runs
        // when an event is queued.
        unsafe {
            if XPending(bar.dpy) == 0 {
                poll(&mut pfd, 1 as c_ulong, timeout as c_int);
            }
            let (mut exposed, mut new_background, mut lower) = (false, false, false);
            while XPending(bar.dpy) > 0 {
                XNextEvent(bar.dpy, &mut ev);
                match ev.kind() {
                    EXPOSE => exposed = true,
                    PROPERTY_NOTIFY => {
                        let (win, prop) = ev.property();
                        new_background |= win == bar.root && bar.root_pmap.contains(&prop);
                    }
                    // A window put under all the others (Window Maker does
                    // that to the lowest of its frames) would cover the
                    // fade, which goes back under it.
                    CONFIGURE_NOTIFY if bar.fade != 0 => {
                        let (win, above) = ev.configure();
                        lower |= if win == bar.fade { above != 0 } else { above == 0 };
                    }
                    _ => {}
                }
            }
            if lower {
                XLowerWindow(bar.dpy, bar.fade);
                XFlush(bar.dpy);
            }
            if new_background {
                bar.shade_background();
                // Painted again at the top of the loop.
                shown = None;
            } else if exposed {
                bar.show();
            }
        }
    }
}
