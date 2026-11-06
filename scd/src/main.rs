// SPDX-License-Identifier: GPL-3.0-or-later
//! scd: give the GUI scale factor of Window Maker (GSScaleFactor) to the
//! applications of the other toolkits, the way GNOME does it.
//!
//! - It becomes the XSETTINGS manager of the screen, for GTK, Chromium and
//!   Electron: Gdk/WindowScalingFactor is the whole part of the factor and
//!   the fonts get the rest of it through Gdk/UnscaledDPI. Xft/DPI keeps the
//!   full resolution for the other readers. GTK and Chromium follow a change
//!   of these settings without being restarted.
//! - It sets the Xft.dpi and Xcursor.size resources (RESOURCE_MANAGER), for
//!   Qt, the applications using Xft directly and the cursor themes. The
//!   previous lines are saved on the root window and put back on exit.
//!
//! Scaling the toolkits one by one (GDK_SCALE, Xft.dpi, ...) is what gets
//! applications scaled twice: Chromium multiplies the GTK window scale by
//! Xft.dpi / 96, so GDK_SCALE=2 with an Xft.dpi of 192 makes it 4 times as
//! big. Gdk/UnscaledDPI is what avoids that.
//!
//! `scd FACTOR` while scd runs sets the factor of the running scd, through a
//! property of its window.

mod xlib;

use std::ffi::CString;
use std::os::raw::{c_int, c_long, c_uchar, c_void};
use std::sync::atomic::{AtomicI32, Ordering};
use std::{env, fs, process, ptr};

use xlib::*;

const USAGE: &str = "usage: scd [-v] [-c SIZE] [FACTOR]";
/// Resolution of an unscaled screen, for the toolkits.
const BASE_DPI: f64 = 96.0;
/// Size of the cursors at factor 1, the one of most desktops.
const DEFAULT_CURSOR_SIZE: u32 = 24;
/// The range accepted by WINGs for GSScaleFactor.
const MIN_FACTOR: f64 = 0.5;
const MAX_FACTOR: f64 = 8.0;
const DPI_RESOURCE: &str = "Xft.dpi";
const CURSOR_RESOURCE: &str = "Xcursor.size";
const RESOURCES: [&str; 2] = [DPI_RESOURCE, CURSOR_RESOURCE];

/// Write end of the pipe the signal handler wakes the main loop with.
static SIGNAL_PIPE: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(_: c_int) {
    let fd = SIGNAL_PIPE.load(Ordering::Relaxed);
    if fd >= 0 {
        // SAFETY: write() is async-signal-safe.
        unsafe { write(fd, b"x".as_ptr() as *const c_void, 1) };
    }
}

fn die(msg: &str) -> ! {
    eprintln!("scd: {msg}");
    process::exit(1)
}

fn parse_factor(s: &str) -> Result<f64, String> {
    match s.trim().parse::<f64>() {
        Ok(f) if f > 0.0 && f.is_finite() => Ok(f.clamp(MIN_FACTOR, MAX_FACTOR)),
        _ => Err(format!("bad scale factor `{s}`")),
    }
}

fn format_factor(f: f64) -> String {
    let s = format!("{f:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// GSScaleFactor from the WMGLOBAL defaults domain, where WINGs reads it.
fn wmglobal_factor() -> Option<f64> {
    let home = env::var("HOME").unwrap_or_default();
    let root = match env::var("WMAKER_USER_ROOT") {
        Ok(dir) if !dir.is_empty() => match dir.strip_prefix("~/") {
            Some(rest) => format!("{home}/{rest}"),
            None => dir,
        },
        _ => format!("{home}/GNUstep"),
    };
    let text = fs::read_to_string(format!("{root}/Defaults/WMGLOBAL")).ok()?;
    // { GSScaleFactor = 2; } or { "GSScaleFactor" = "1.5"; }
    let i = text.find("GSScaleFactor")?;
    let rest = text[i + "GSScaleFactor".len()..].trim_start_matches('"').trim_start();
    let rest = rest.strip_prefix('=')?.trim_start().trim_start_matches('"');
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(rest.len());
    parse_factor(&rest[..end]).ok()
}

/// The factor Window Maker uses: GS_SCALE_FACTOR, then GSScaleFactor.
fn default_factor() -> f64 {
    env::var("GS_SCALE_FACTOR")
        .ok()
        .and_then(|s| parse_factor(&s).ok())
        .or_else(wmglobal_factor)
        .unwrap_or(1.0)
}

/// Whether a resource line sets `key` (`Xft.dpi:\t96`).
fn sets(line: &str, key: &str) -> bool {
    line.trim_start().strip_prefix(key).is_some_and(|r| r.trim_start().starts_with(':'))
}

/// The lines of `resources` that set one of `keys` (`keep`), or the others.
fn filter_lines(resources: &str, keys: &[&str], keep: bool) -> String {
    let mut out = String::new();
    for line in resources.lines() {
        if keys.iter().any(|k| sets(line, k)) == keep && !line.is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// The XSETTINGS property: integer settings only.
fn xsettings_data(serial: u32, settings: &[(&str, i32)]) -> Vec<u8> {
    let mut d = vec![if cfg!(target_endian = "big") { 1 } else { 0 }, 0, 0, 0];
    d.extend(serial.to_ne_bytes());
    d.extend((settings.len() as u32).to_ne_bytes());
    for (name, value) in settings {
        d.extend([0, 0]); // XSettingsTypeInteger, pad
        d.extend((name.len() as u16).to_ne_bytes());
        d.extend(name.as_bytes());
        d.resize(d.len().next_multiple_of(4), 0);
        d.extend(serial.to_ne_bytes()); // last change
        d.extend(value.to_ne_bytes());
    }
    d
}

struct Scd {
    dpy: *mut Display,
    root: Window,
    win: Window,
    verbose: bool,
    cursor_size: u32,
    selection: Atom,
    settings_atom: Atom,
    factor_atom: Atom,
    saved_atom: Atom,
    manager_atom: Atom,
    serial: u32,
    factor: f64,
}

impl Scd {
    fn atom(dpy: *mut Display, name: &str) -> Atom {
        let name = CString::new(name).unwrap();
        unsafe { XInternAtom(dpy, name.as_ptr(), FALSE) }
    }

    /// A STRING property, `None` if the window does not have it.
    fn get_string(&self, win: Window, prop: Atom) -> Option<String> {
        let (mut type_, mut format, mut n, mut after) = (0, 0, 0, 0);
        let mut data: *mut c_uchar = ptr::null_mut();
        let status = unsafe {
            XGetWindowProperty(
                self.dpy, win, prop, 0, 1 << 20, FALSE, XA_STRING, &mut type_, &mut format, &mut n,
                &mut after, &mut data,
            )
        };
        if status != SUCCESS || type_ == NONE {
            return None;
        }
        let mut s = String::new();
        if !data.is_null() {
            if format == 8 {
                // SAFETY: Xlib returns `n` items of 8 bits.
                let bytes = unsafe { std::slice::from_raw_parts(data, n as usize) };
                s = String::from_utf8_lossy(bytes).into_owned();
            }
            unsafe { XFree(data as *mut c_void) };
        }
        Some(s)
    }

    fn set_property(&self, win: Window, prop: Atom, type_: Atom, data: &[u8]) {
        unsafe {
            XChangeProperty(self.dpy, win, prop, type_, 8, PROP_MODE_REPLACE, data.as_ptr(), data.len() as c_int)
        };
    }

    /// Replaces our lines of RESOURCE_MANAGER, after saving the ones that
    /// were there before scd (once: a scd that was killed left its own).
    fn set_resources(&self, values: &[(&str, String)]) {
        unsafe { XGrabServer(self.dpy) };
        let current = self.get_string(self.root, XA_RESOURCE_MANAGER).unwrap_or_default();
        if self.get_string(self.root, self.saved_atom).is_none() {
            let saved = filter_lines(&current, &RESOURCES, true);
            self.set_property(self.root, self.saved_atom, XA_STRING, saved.as_bytes());
        }
        let mut new = filter_lines(&current, &RESOURCES, false);
        for (key, value) in values {
            new.push_str(&format!("{key}:\t{value}\n"));
        }
        self.set_property(self.root, XA_RESOURCE_MANAGER, XA_STRING, new.as_bytes());
        unsafe {
            XUngrabServer(self.dpy);
            XFlush(self.dpy);
        }
    }

    fn restore_resources(&self) {
        unsafe { XGrabServer(self.dpy) };
        if let Some(saved) = self.get_string(self.root, self.saved_atom) {
            let current = self.get_string(self.root, XA_RESOURCE_MANAGER).unwrap_or_default();
            let new = filter_lines(&current, &RESOURCES, false) + &saved;
            if new.is_empty() {
                unsafe { XDeleteProperty(self.dpy, self.root, XA_RESOURCE_MANAGER) };
            } else {
                self.set_property(self.root, XA_RESOURCE_MANAGER, XA_STRING, new.as_bytes());
            }
            unsafe { XDeleteProperty(self.dpy, self.root, self.saved_atom) };
        }
        unsafe {
            XUngrabServer(self.dpy);
            XSync(self.dpy, FALSE);
        }
    }

    fn apply(&mut self) {
        let scale = ((self.factor + 0.01).floor() as i32).max(1);
        let dpi = BASE_DPI * self.factor;
        let unscaled = (dpi / scale as f64 * 1024.0).round() as i32;
        let cursor = (self.cursor_size as f64 * self.factor).round() as u32;

        self.serial = self.serial.wrapping_add(1);
        let data = xsettings_data(
            self.serial,
            &[
                ("Gdk/WindowScalingFactor", scale),
                ("Gdk/UnscaledDPI", unscaled),
                ("Xft/DPI", (dpi * 1024.0).round() as i32),
            ],
        );
        self.set_property(self.win, self.settings_atom, self.settings_atom, &data);
        self.set_resources(&[
            (DPI_RESOURCE, format!("{}", dpi.round() as u32)),
            (CURSOR_RESOURCE, format!("{cursor}")),
        ]);
        if self.verbose {
            eprintln!(
                "scd: factor {}: window scale {scale}, fonts {:.0} dpi, cursors {cursor}",
                format_factor(self.factor),
                unscaled as f64 / 1024.0
            );
        }
    }

    /// Takes the XSETTINGS selection and tells the clients (ICCCM manager
    /// selection, with a timestamp taken from a property change).
    fn acquire(&mut self) -> Result<(), String> {
        let value = format_factor(self.factor);
        self.set_property(self.win, self.factor_atom, XA_STRING, value.as_bytes());
        let mut ev = XEvent::zeroed();
        let time = loop {
            unsafe { XNextEvent(self.dpy, &mut ev) };
            if ev.kind() == PROPERTY_NOTIFY {
                // SAFETY: checked the type.
                let p: &XPropertyEvent = unsafe { ev.as_ref() };
                if p.window == self.win && p.atom == self.factor_atom {
                    break p.time;
                }
            }
        };

        unsafe { XSetSelectionOwner(self.dpy, self.selection, self.win, time) };
        if unsafe { XGetSelectionOwner(self.dpy, self.selection) } != self.win {
            return Err("could not become the XSETTINGS manager".into());
        }
        self.apply();

        let msg = XClientMessageEvent {
            type_: CLIENT_MESSAGE,
            serial: 0,
            send_event: 1,
            display: self.dpy,
            window: self.root,
            message_type: self.manager_atom,
            format: 32,
            data: [time as c_long, self.selection as c_long, self.win as c_long, 0, 0],
        };
        let mut ev = XEvent::zeroed();
        // SAFETY: XClientMessageEvent is a member of the union, smaller than it.
        unsafe {
            ptr::write(&mut ev as *mut XEvent as *mut XClientMessageEvent, msg);
            XSendEvent(self.dpy, self.root, FALSE, STRUCTURE_NOTIFY_MASK, &mut ev);
            XFlush(self.dpy);
        }
        Ok(())
    }

    /// Handles one event; false when scd has to stop.
    fn handle(&mut self, ev: &XEvent) -> bool {
        match ev.kind() {
            PROPERTY_NOTIFY => {
                // SAFETY: checked the type.
                let p: &XPropertyEvent = unsafe { ev.as_ref() };
                if p.window != self.win || p.atom != self.factor_atom || p.state != PROPERTY_NEW_VALUE {
                    return true;
                }
                let Some(value) = self.get_string(self.win, self.factor_atom) else {
                    return true;
                };
                match parse_factor(&value) {
                    Ok(f) if (f - self.factor).abs() > 1e-6 => {
                        self.factor = f;
                        self.apply();
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("scd: {e}"),
                }
            }
            SELECTION_CLEAR => {
                // SAFETY: checked the type.
                let s: &XSelectionClearEvent = unsafe { ev.as_ref() };
                if s.selection == self.selection {
                    eprintln!("scd: another XSETTINGS manager took over");
                    return false;
                }
            }
            _ => {}
        }
        true
    }

    fn run(&mut self) {
        let mut fds = [0 as c_int; 2];
        if unsafe { pipe(fds.as_mut_ptr()) } != 0 {
            die("pipe failed");
        }
        SIGNAL_PIPE.store(fds[1], Ordering::Relaxed);
        for sig in [SIGHUP, SIGINT, SIGTERM] {
            unsafe { signal(sig, on_signal) };
        }

        let xfd = unsafe { XConnectionNumber(self.dpy) };
        let mut ev = XEvent::zeroed();
        'run: loop {
            while unsafe { XPending(self.dpy) } > 0 {
                unsafe { XNextEvent(self.dpy, &mut ev) };
                if !self.handle(&ev) {
                    break 'run;
                }
            }
            let mut pfd = [
                PollFd { fd: xfd, events: POLLIN, revents: 0 },
                PollFd { fd: fds[0], events: POLLIN, revents: 0 },
            ];
            if unsafe { poll(pfd.as_mut_ptr(), 2, -1) } < 0 {
                if std::io::Error::last_os_error().raw_os_error() == Some(EINTR) {
                    continue;
                }
                die("poll failed");
            }
            if pfd[1].revents != 0 {
                break;
            }
        }

        self.restore_resources();
        unsafe {
            XDestroyWindow(self.dpy, self.win);
            XCloseDisplay(self.dpy);
        }
    }
}

fn main() {
    let mut verbose = false;
    let mut cursor_size = DEFAULT_CURSOR_SIZE;
    let mut factor = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-v" => verbose = true,
            "-c" => {
                cursor_size = match args.next().and_then(|s| s.parse().ok()) {
                    Some(n) if n > 0 => n,
                    _ => die(USAGE),
                }
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ if factor.is_none() && !arg.starts_with('-') => {
                factor = Some(parse_factor(&arg).unwrap_or_else(|e| die(&e)))
            }
            _ => die(USAGE),
        }
    }

    let dpy = unsafe { XOpenDisplay(ptr::null()) };
    if dpy.is_null() {
        die("cannot open the display");
    }
    let screen = unsafe { XDefaultScreen(dpy) };
    let root = unsafe { XRootWindow(dpy, screen) };
    let mut scd = Scd {
        dpy,
        root,
        win: NONE,
        verbose,
        cursor_size,
        selection: Scd::atom(dpy, &format!("_XSETTINGS_S{screen}")),
        settings_atom: Scd::atom(dpy, "_XSETTINGS_SETTINGS"),
        factor_atom: Scd::atom(dpy, "_SCD_FACTOR"),
        saved_atom: Scd::atom(dpy, "_SCD_SAVED_RESOURCES"),
        manager_atom: Scd::atom(dpy, "MANAGER"),
        serial: 0,
        factor: factor.unwrap_or_else(default_factor),
    };

    let owner = unsafe { XGetSelectionOwner(dpy, scd.selection) };
    if owner != NONE {
        if scd.get_string(owner, scd.factor_atom).is_none() {
            die("another XSETTINGS manager runs (xsettingsd, a desktop settings daemon): stop it first");
        }
        let Some(f) = factor else {
            die("scd already runs; give a FACTOR to change its scale factor");
        };
        // the running scd watches this property of its window
        scd.set_property(owner, scd.factor_atom, XA_STRING, format_factor(f).as_bytes());
        unsafe { XSync(dpy, FALSE) };
        return;
    }

    scd.win = unsafe { XCreateSimpleWindow(dpy, root, -1, -1, 1, 1, 0, 0, 0) };
    unsafe { XSelectInput(dpy, scd.win, PROPERTY_CHANGE_MASK) };
    if let Err(e) = scd.acquire() {
        die(&e);
    }
    scd.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_lines() {
        let res = "Xft.dpi:\t120\nXft.antialias: 1\n  Xcursor.size : 32\nXft.dpix: 3\n";
        assert_eq!(filter_lines(res, &RESOURCES, true), "Xft.dpi:\t120\n  Xcursor.size : 32\n");
        assert_eq!(filter_lines(res, &RESOURCES, false), "Xft.antialias: 1\nXft.dpix: 3\n");
    }

    #[test]
    fn factors() {
        assert_eq!(parse_factor(" 1.5"), Ok(1.5));
        assert_eq!(parse_factor("20"), Ok(MAX_FACTOR));
        assert!(parse_factor("x").is_err() && parse_factor("0").is_err() && parse_factor("NaN").is_err());
        assert_eq!(format_factor(2.0), "2");
        assert_eq!(format_factor(1.25), "1.25");
    }

    #[test]
    fn xsettings_layout() {
        let d = xsettings_data(7, &[("Xft/DPI", 196608)]);
        // header 12, entry 4 + name padded to 8 + serial 4 + value 4
        assert_eq!(d.len(), 12 + 4 + 8 + 4 + 4);
        assert_eq!(&d[4..8], &7u32.to_ne_bytes());
        assert_eq!(&d[16..23], b"Xft/DPI");
        assert_eq!(&d[28..32], &196608i32.to_ne_bytes());
    }
}
