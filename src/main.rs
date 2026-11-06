// SPDX-License-Identifier: GPL-3.0-or-later
//! mediakeyd: run commands for media keys without grabbing them.
//!
//! Hotkey daemons such as sxhkd use XGrabKey. Whenever a grabbed key is
//! pressed, the X server sends FocusOut/FocusIn (NotifyGrab/NotifyUngrab) to
//! the focused window, and GTK applications such as LibreWolf take that as a
//! real focus change: menus close and the window turns inactive for a moment.
//!
//! mediakeyd only listens to XInput2 raw key events on the root window. No
//! key is grabbed, so no focus events are generated; the focused application
//! still receives the key as usual.
//!
//! Raw events carry no autorepeat, so keys marked `repeat` are repeated here
//! with the X server's repeat delay and rate for as long as they are down.
//! A key's command never runs twice at the same time: presses that arrive
//! while it runs are merged into one more run after it finishes.

mod xlib;

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uint, c_ulong};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, fs, process, ptr};

use xlib::*;

const USAGE: &str = "usage: mediakeyd [-v] [-c CONFIG]";
/// How often to look for finished commands while one is running.
const CHILD_POLL: Duration = Duration::from_millis(5);
/// Used when the X server does not report its repeat settings.
const DEFAULT_REPEAT: (Duration, Duration) = (Duration::from_millis(660), Duration::from_millis(40));

struct Binding {
    name: String,
    keysym: KeySym,
    repeat: bool,
    command: String,
    child: Option<Child>,
    pending: bool,
}

/// A `repeat` key that is down.
struct Held {
    keycode: u8,
    binding: usize,
    next: Instant,
}

fn die(msg: &str) -> ! {
    eprintln!("mediakeyd: {msg}");
    process::exit(1)
}

fn default_config_path() -> String {
    let base = match env::var("XDG_CONFIG_HOME") {
        Ok(dir) if !dir.is_empty() => dir,
        _ => format!("{}/.config", env::var("HOME").unwrap_or_default()),
    };
    format!("{base}/mediakeyd/config")
}

/// Splits off the first whitespace-separated word.
fn split_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// Config lines: `KEYSYM repeat|once COMMAND`, `#` starts a comment line.
fn parse_config(path: &str) -> Result<Vec<Binding>, String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut bindings = Vec::new();

    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |what: &str| format!("{path}:{}: {what}", n + 1);
        let (name, rest) = split_word(line);
        let (mode, command) = split_word(rest);
        let repeat = match mode {
            "repeat" => true,
            "once" => false,
            _ => return Err(err("expected `repeat` or `once` after the keysym")),
        };
        if command.is_empty() {
            return Err(err("missing command"));
        }
        let cname = CString::new(name).map_err(|_| err("bad keysym"))?;
        let keysym = unsafe { XStringToKeysym(cname.as_ptr()) };
        if keysym == 0 {
            return Err(err(&format!("unknown keysym {name}")));
        }
        bindings.push(Binding {
            name: name.to_string(),
            keysym,
            repeat,
            command: command.to_string(),
            child: None,
            pending: false,
        });
    }
    Ok(bindings)
}

/// Asks for raw key press/release events of all master keyboards on the root
/// window and returns the XInput major opcode.
fn select_raw_keys(dpy: *mut Display) -> Result<c_int, &'static str> {
    let (mut opcode, mut event, mut error) = (0, 0, 0);
    let (mut major, mut minor) = (2, 2);
    unsafe {
        if XQueryExtension(dpy, c"XInputExtension".as_ptr(), &mut opcode, &mut event, &mut error) == 0 {
            return Err("the X server has no XInput extension");
        }
        // Since XInput 2.1 raw events reach the root window even while
        // another client holds a grab.
        if XIQueryVersion(dpy, &mut major, &mut minor) != SUCCESS || (major, minor) < (2, 1) {
            return Err("the X server does not support XInput 2.1");
        }
        let mut mask = [0u8; 4];
        for ev in [XI_RAW_KEY_PRESS, XI_RAW_KEY_RELEASE] {
            mask[(ev >> 3) as usize] |= 1 << (ev & 7);
        }
        let mut em = XIEventMask {
            deviceid: XI_ALL_MASTER_DEVICES,
            mask_len: mask.len() as c_int,
            mask: mask.as_mut_ptr(),
        };
        XISelectEvents(dpy, XDefaultRootWindow(dpy), &mut em, 1);
    }
    Ok(opcode)
}

/// Maps keycodes to bindings with the current keyboard map.
fn keycode_table(dpy: *mut Display, bindings: &[Binding]) -> [Option<usize>; 256] {
    let mut table = [None; 256];
    for (i, b) in bindings.iter().enumerate() {
        match unsafe { XKeysymToKeycode(dpy, b.keysym) } {
            0 => eprintln!("mediakeyd: {} is not on the keyboard map", b.name),
            kc => table[kc as usize] = Some(i),
        }
    }
    table
}

/// Returns `(evtype, keycode)` for an XInput raw key event.
fn raw_key(dpy: *mut Display, ev: &mut XEvent, opcode: c_int) -> Option<(c_int, u8)> {
    let cookie = ev as *mut XEvent as *mut XGenericEventCookie;
    unsafe {
        let evtype = (*cookie).evtype;
        if (*cookie).extension != opcode || (evtype != XI_RAW_KEY_PRESS && evtype != XI_RAW_KEY_RELEASE) {
            return None;
        }
        if XGetEventData(dpy, cookie) == 0 {
            return None;
        }
        let detail = (*((*cookie).data as *const XIRawEvent)).detail;
        XFreeEventData(dpy, cookie);
        u8::try_from(detail).ok().map(|kc| (evtype, kc))
    }
}

/// The X server's autorepeat delay and interval (`xset r rate`).
fn repeat_rate(dpy: *mut Display) -> (Duration, Duration) {
    let (mut delay, mut interval): (c_uint, c_uint) = (0, 0);
    let ok = unsafe { XkbGetAutoRepeatRate(dpy, XKB_USE_CORE_KBD, &mut delay, &mut interval) };
    if ok == 0 || interval == 0 {
        return DEFAULT_REPEAT;
    }
    (Duration::from_millis(delay.into()), Duration::from_millis(interval.into()))
}

fn keys_down(dpy: *mut Display) -> [u8; 32] {
    let mut keys = [0 as c_char; 32];
    unsafe { XQueryKeymap(dpy, keys.as_mut_ptr()) };
    keys.map(|b| b as u8)
}

fn is_down(keys: &[u8; 32], keycode: u8) -> bool {
    keys[(keycode >> 3) as usize] >> (keycode & 7) & 1 == 1
}

fn spawn(b: &Binding) -> Option<Child> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(&b.command)
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| eprintln!("mediakeyd: {}: {e}", b.name))
        .ok()
}

/// Runs the binding's command, or queues one more run if it is still running.
fn fire(b: &mut Binding) {
    if b.child.is_some() {
        b.pending = true;
    } else {
        b.child = spawn(b);
    }
}

/// Collects finished commands and starts queued runs.
fn reap(bindings: &mut [Binding]) {
    for b in bindings {
        let Some(child) = &mut b.child else { continue };
        if matches!(child.try_wait(), Ok(None)) {
            continue;
        }
        b.child = None;
        if b.pending {
            b.pending = false;
            b.child = spawn(b);
        }
    }
}

fn main() {
    let mut verbose = false;
    let mut path = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-v" => verbose = true,
            "-c" => path = Some(args.next().unwrap_or_else(|| die(USAGE))),
            _ => die(USAGE),
        }
    }
    let path = path.unwrap_or_else(default_config_path);
    let mut bindings = parse_config(&path).unwrap_or_else(|e| die(&e));
    if bindings.is_empty() {
        die(&format!("{path}: no bindings"));
    }

    let dpy = unsafe { XOpenDisplay(ptr::null()) };
    if dpy.is_null() {
        die("cannot open the X display");
    }
    let opcode = select_raw_keys(dpy).unwrap_or_else(|e| die(e));
    let mut table = keycode_table(dpy, &bindings);
    let mut held: Vec<Held> = Vec::new();
    let fd = unsafe { XConnectionNumber(dpy) };

    loop {
        while unsafe { XPending(dpy) } > 0 {
            let mut ev = XEvent::zeroed();
            unsafe { XNextEvent(dpy, &mut ev) };
            match ev.kind() {
                MAPPING_NOTIFY => {
                    unsafe { XRefreshKeyboardMapping(&mut ev) };
                    table = keycode_table(dpy, &bindings);
                    held.clear();
                }
                GENERIC_EVENT => {
                    let Some((evtype, keycode)) = raw_key(dpy, &mut ev, opcode) else { continue };
                    let Some(i) = table[keycode as usize] else { continue };
                    if evtype == XI_RAW_KEY_RELEASE {
                        held.retain(|h| h.keycode != keycode);
                        continue;
                    }
                    if held.iter().any(|h| h.keycode == keycode) {
                        continue;
                    }
                    if verbose {
                        eprintln!("mediakeyd: {}", bindings[i].name);
                    }
                    fire(&mut bindings[i]);
                    if bindings[i].repeat {
                        let (delay, _) = repeat_rate(dpy);
                        held.push(Held { keycode, binding: i, next: Instant::now() + delay });
                    }
                }
                _ => {}
            }
        }

        reap(&mut bindings);

        let now = Instant::now();
        if held.iter().any(|h| h.next <= now) {
            // A release can be missed (VT switch, server reset), so check
            // that the key is really still down before repeating it.
            let keys = keys_down(dpy);
            held.retain(|h| is_down(&keys, h.keycode));
            let (_, interval) = repeat_rate(dpy);
            for h in held.iter_mut().filter(|h| h.next <= now) {
                fire(&mut bindings[h.binding]);
                h.next = (h.next + interval).max(now + interval / 2);
            }
        }

        // The round trips above may have queued events without them
        // showing up on the socket.
        if unsafe { XPending(dpy) } > 0 {
            continue;
        }

        let now = Instant::now();
        let mut wait = held.iter().map(|h| h.next.saturating_duration_since(now)).min();
        if bindings.iter().any(|b| b.child.is_some()) {
            wait = Some(wait.map_or(CHILD_POLL, |w| w.min(CHILD_POLL)));
        }
        let timeout = wait.map_or(-1, |w| w.as_micros().div_ceil(1000).min(c_int::MAX as u128) as c_int);
        let mut pfd = PollFd { fd, events: POLLIN, revents: 0 };
        unsafe { poll(&mut pfd, 1 as c_ulong, timeout) };
    }
}
