// SPDX-License-Identifier: GPL-3.0-or-later
//! autobrightd: ambient-light auto-brightness for Apple Silicon Macs on
//! Asahi Linux. Drives the screen and keyboard backlights from the AOP
//! ambient light sensor and steps aside when someone else changes them.
//!
//! It wakes 5 times a second while nothing changes (the sensor reports every
//! 200 ms), and about 21 times a second while a change runs. Manual
//! screen changes are noticed through sysfs_notify() on actual_brightness,
//! and plugging or unplugging the charger through kernel uevents.

/// eprintln! that ignores errors. With panic = "abort", a closed or broken
/// stderr (say, the terminal that started the session is gone) would
/// otherwise kill the daemon at its next message.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

mod logic;
mod sys;

use logic::{Channel, LuxFilter, Settings, Update};
use std::path::Path;
use std::process::exit;
use std::time::{Duration, Instant};
use sys::{Attr, Uevents};

const ALS_NAME: &str = "aop-sensors-als";
const SCREEN_DIR: &str = "/sys/class/backlight/apple-panel-bl";
const KBD_DIR: &str = "/sys/class/leds/kbd_backlight";
/// Whether the charger is connected. macsmc-ac/online cannot tell: on an
/// M1 Pro it drops to 0 for about 0.4 s when the cable goes in or out and
/// reads 1 otherwise. The battery status is right, but each read costs about
/// 0.8 ms of SMC traffic, so it is read only when the SMC driver sends a
/// power_supply uevent (on every plug or unplug).
const BATTERY_STATUS: &str = "/sys/class/power_supply/macsmc-battery/status";
const POWER_SUPPLY_PREFIX: &str = "macsmc-";
/// The status is read again this long after an event, in case it lagged.
const POWER_RECHECK: Duration = Duration::from_millis(1500);
/// Without uevents, the status is polled this often instead.
const POWER_POLL: Duration = Duration::from_secs(5);

/// The sensor reports every 200 ms; reading it more often finds the same value.
const SAMPLE: Duration = Duration::from_millis(200);
/// While the panel is off, only DPMS is checked, this often.
const DPMS_OFF_CHECK: Duration = Duration::from_secs(1);

const USAGE: &str = "usage: autobrightd [-v] [-n]
  -v  print what it does
  -n  dry run: read the sensor and print, but write nothing";

struct Backlight {
    channel: Channel,
    attr: Attr,
}

struct Daemon {
    als: Attr,
    screen: Backlight,
    /// actual_brightness of the screen, only for its change notification.
    screen_notify: Option<Attr>,
    kbd: Option<Backlight>,
    battery: Option<Attr>,
    uevents: Option<Uevents>,
    /// When to read the battery status next, if at all.
    power_check: Option<Instant>,
    dpms: Option<Attr>,
    verbose: bool,
    dry_run: bool,
    filter: LuxFilter,
    lux: f32,
    on_ac: bool,
    panel_off: bool,
}

fn open_backlight(name: &'static str, dir: &str, settings: Settings, write: bool) -> Result<Backlight, String> {
    let attr = Attr::open(Path::new(dir).join("brightness"), write).map_err(|e| {
        let hint = if e.kind() == std::io::ErrorKind::PermissionDenied {
            "\n  install 90-autobrightd.rules (see README) and be in the video group, or use -n"
        } else {
            ""
        };
        format!("{dir}/brightness: {e}{hint}")
    })?;
    let current = attr.read_u32().map_err(|e| format!("{}: {e}", attr.path.display()))?;
    Ok(Backlight { channel: Channel::new(name, settings, current), attr })
}

fn max_brightness(dir: &str) -> Result<u32, String> {
    let path = Path::new(dir).join("max_brightness");
    sys::read_once(&path)
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&m| m > 0)
        .ok_or_else(|| format!("cannot read {}", path.display()))
}

impl Daemon {
    fn open(verbose: bool, dry_run: bool) -> Result<Daemon, String> {
        let write = !dry_run;
        let als_dir = sys::find_iio(ALS_NAME).ok_or_else(|| {
            format!(
                "no IIO device named {ALS_NAME}\n  is /lib/firmware/apple/aop-als-cal.bin installed? \
                 If it was added after boot, reboot once"
            )
        })?;
        let als = Attr::open(als_dir.join("in_illuminance_input"), false)
            .map_err(|e| format!("{}: {e}", als_dir.display()))?;

        let screen_settings = Settings {
            curve: logic::SCREEN_CURVE,
            floor_pct: logic::SCREEN_FLOOR_PCT,
            motion: logic::SCREEN_MOTION,
            max: max_brightness(SCREEN_DIR)?,
        };
        let screen = open_backlight("screen", SCREEN_DIR, screen_settings, write)?;
        let screen_notify = Attr::open(Path::new(SCREEN_DIR).join("actual_brightness"), false).ok();
        // Read once so the notification is armed.
        if let Some(n) = &screen_notify {
            let _ = n.read_int();
        }

        let kbd = match max_brightness(KBD_DIR) {
            Ok(max) => {
                let settings = Settings {
                    curve: logic::KBD_CURVE,
                    floor_pct: logic::KBD_FLOOR_PCT,
                    max,
                    motion: logic::KBD_MOTION,
                };
                Some(open_backlight("kbd", KBD_DIR, settings, write)?)
            }
            Err(_) => None,
        };
        let battery = Attr::open(BATTERY_STATUS, false).ok();
        let uevents = battery.as_ref().and_then(|_| Uevents::open().ok());
        let dpms = sys::find_edp_dpms().and_then(|p| Attr::open(p, false).ok());

        let d = Daemon {
            als,
            screen,
            screen_notify,
            kbd,
            power_check: battery.as_ref().map(|_| Instant::now()),
            battery,
            uevents,
            dpms,
            verbose,
            dry_run,
            filter: LuxFilter::default(),
            lux: 0.0,
            on_ac: false,
            panel_off: false,
        };
        if verbose {
            let path = |a: &Option<Attr>| a.as_ref().map_or("(none)".into(), |a| a.path.display().to_string());
            say!("autobrightd: als {}", d.als.path.display());
            say!("autobrightd: screen {} max {}", d.screen.attr.path.display(), d.screen.channel.settings.max);
            match &d.kbd {
                Some(k) => say!("autobrightd: kbd {} max {}", k.attr.path.display(), k.channel.settings.max),
                None => say!("autobrightd: kbd (none)"),
            }
            let how = if d.uevents.is_some() { "on uevents" } else { "polled" };
            say!("autobrightd: power {} ({how})", path(&d.battery));
            say!("autobrightd: dpms {}, notify {}", path(&d.dpms), path(&d.screen_notify));
            if dry_run {
                say!("autobrightd: dry run, nothing is written");
            }
        }
        Ok(d)
    }

    fn log(&self, msg: std::fmt::Arguments) {
        if self.verbose {
            say!("autobrightd: {msg}");
        }
    }

    fn backlights(&mut self) -> impl Iterator<Item = &mut Backlight> {
        std::iter::once(&mut self.screen).chain(self.kbd.as_mut())
    }

    /// Reads a backlight and notes a change made by someone else.
    fn check(&mut self, which: Which) {
        if self.dry_run {
            return;
        }
        let lux = self.lux;
        let verbose = self.verbose;
        let Some(b) = self.get(which) else { return };
        if let Ok(v) = b.attr.read_u32()
            && b.channel.observe(v, lux)
            && verbose
        {
            say!("autobrightd: {} set to {v} by someone else, holding until the light changes", b.channel.name);
        }
    }

    fn get(&mut self, which: Which) -> Option<&mut Backlight> {
        match which {
            Which::Screen => Some(&mut self.screen),
            Which::Kbd => self.kbd.as_mut(),
        }
    }

    /// Reads the battery status: anything but Discharging means the charger
    /// is connected. The next sample applies the new AC boost.
    fn check_power(&mut self) {
        let Some(battery) = &self.battery else { return };
        let mut buf = [0; 32];
        let status = match battery.read_str(&mut buf) {
            Ok(s) => s.to_owned(),
            Err(e) => {
                self.log(format_args!("reading {}: {e}", battery.path.display()));
                return;
            }
        };
        let on = status != "Discharging";
        if on != self.on_ac {
            self.on_ac = on;
            self.log(format_args!("{} (battery {status})", if on { "on AC" } else { "on battery" }));
        }
    }

    /// One sensor sample: DPMS, lux, keyboard check, new targets.
    fn sample(&mut self, now: Instant) {
        let off = self.dpms.as_ref().is_some_and(|d| {
            let mut buf = [0; 32];
            d.read_str(&mut buf).is_ok_and(|s| s != "On")
        });
        if off != self.panel_off {
            self.panel_off = off;
            self.log(format_args!("panel {}", if off { "off" } else { "on" }));
            if off {
                self.panel_turned_off();
            }
        }
        if off {
            return;
        }

        let raw = match self.als.read_int() {
            Ok(v) => v as f32,
            Err(e) => {
                self.log(format_args!("reading lux: {e}"));
                return;
            }
        };
        self.lux = self.filter.update(raw);
        // The LED class sends no change notification, so look each sample.
        self.check(Which::Kbd);

        let lux = self.lux;
        let boost = if self.on_ac { logic::AC_SCREEN_BOOST_PCT } else { 0.0 };
        let verbose = self.verbose;
        for b in self.backlights() {
            let boost = if b.channel.name == "screen" { boost } else { 0.0 };
            let update = b.channel.update(lux, boost, now);
            if !verbose {
                continue;
            }
            if update == Update::Resumed {
                say!("autobrightd: {}: light changed (lux {lux:.0}), automatic again", b.channel.name);
            }
            if let Update::Moving(to) = update {
                say!(
                    "autobrightd: lux {raw:.0} (smoothed {lux:.0}): {} {} -> {to}",
                    b.channel.name, b.channel.known
                );
            }
        }
    }

    /// Panel blanked: switch the keyboard off at once and stop all ramps.
    /// Screen writes would only wake the DCP for nothing; the kernel keeps
    /// the value for when the panel comes back.
    fn panel_turned_off(&mut self) {
        self.filter.reset();
        self.screen.channel.stop();
        let dry_run = self.dry_run;
        if let Some(k) = &mut self.kbd {
            k.channel.stop();
            k.channel.override_lux = None;
            if k.channel.known > 0 && (dry_run || k.attr.write_u32(0).is_ok()) {
                k.channel.wrote(0);
                k.channel.stop();
            }
        }
    }

    /// Writes ramp frames that are due.
    fn frames(&mut self, now: Instant) {
        for which in [Which::Screen, Which::Kbd] {
            let due = self.get(which).and_then(|b| b.channel.next_frame()).is_some_and(|t| t <= now);
            if !due {
                continue;
            }
            // Someone may have changed it since our last write.
            self.check(which);
            let (dry_run, verbose, lux) = (self.dry_run, self.verbose, self.lux);
            let Some(b) = self.get(which) else { continue };
            let Some(value) = b.channel.frame(now) else { continue };
            if dry_run {
                b.channel.wrote(value);
                if verbose {
                    say!("autobrightd: would write {} {value}", b.channel.name);
                }
                continue;
            }
            let started = Instant::now();
            match b.attr.write_u32(value) {
                Ok(()) => {
                    b.channel.wrote(value);
                    // Each write holds the backlight lock for about 45 ms. A
                    // key press that waited for it may already have replaced
                    // our value; if so, that is a manual change.
                    let stored = b.attr.read_u32().unwrap_or(value);
                    b.channel.not_before(Instant::now() + logic::WRITE_GAP);
                    if verbose {
                        let ms = started.elapsed().as_millis();
                        say!("autobrightd: {} {value} ({ms} ms)", b.channel.name);
                    }
                    if stored != value && b.channel.observe(stored, lux) && verbose {
                        say!(
                            "autobrightd: {} set to {stored} by someone else, holding until the light changes",
                            b.channel.name
                        );
                    }
                }
                Err(e) => {
                    say!("autobrightd: writing {}: {e}", b.attr.path.display());
                    b.channel.stop();
                }
            }
        }
    }

    fn run(&mut self) -> ! {
        let mut next_sample = Instant::now();
        loop {
            let now = Instant::now();
            let mut deadline = next_sample;
            if !self.panel_off {
                for b in self.backlights() {
                    if let Some(t) = b.channel.next_frame() {
                        deadline = deadline.min(t);
                    }
                }
            }
            if let Some(t) = self.power_check {
                deadline = deadline.min(t);
            }
            let notify = self.screen_notify.as_ref().map(Attr::fd);
            let uevent = self.uevents.as_ref().map(Uevents::fd);
            let ready = match sys::wait(notify, uevent, deadline.saturating_duration_since(now)) {
                Ok(r) => r,
                Err(e) => {
                    say!("autobrightd: poll: {e}");
                    exit(1);
                }
            };
            if ready.notify {
                // Re-arm, then see whether it was us.
                if let Some(n) = &self.screen_notify {
                    let _ = n.read_int();
                }
                self.check(Which::Screen);
            }

            let now = Instant::now();
            if ready.uevent && self.uevents.as_mut().is_some_and(|u| u.drain(POWER_SUPPLY_PREFIX)) {
                self.check_power();
                self.power_check = Some(now + POWER_RECHECK);
            }
            if self.power_check.is_some_and(|t| t <= now) {
                self.check_power();
                self.power_check = if self.uevents.is_some() { None } else { Some(now + POWER_POLL) };
            }
            if now >= next_sample {
                self.sample(now);
                next_sample = if self.panel_off {
                    now + DPMS_OFF_CHECK
                } else if next_sample + SAMPLE > now {
                    next_sample + SAMPLE
                } else {
                    now + SAMPLE
                };
            }
            if !self.panel_off {
                self.frames(Instant::now());
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Which {
    Screen,
    Kbd,
}

fn main() {
    let mut verbose = false;
    let mut dry_run = false;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-v" => verbose = true,
            "-n" => dry_run = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            _ => {
                say!("{USAGE}");
                exit(2);
            }
        }
    }
    match Daemon::open(verbose, dry_run) {
        Ok(mut d) => d.run(),
        Err(e) => {
            say!("autobrightd: {e}");
            exit(1);
        }
    }
}
