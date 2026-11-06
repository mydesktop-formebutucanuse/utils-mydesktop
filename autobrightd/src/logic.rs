// SPDX-License-Identifier: GPL-3.0-or-later AND MIT
//! Brightness logic without any I/O: curves, lux smoothing, the dead band,
//! ramps and the manual-override rule.
//!
//! The curve points and the override rule come from asahi-brightnessd
//! (Craig Hughes) and juicecultus/asahi-auto-brightness, both MIT licensed;
//! see LICENSE-MIT.

use std::time::{Duration, Instant};

/// Points of lux → percent of max_brightness, joined by straight lines.
pub type Curve = &'static [(f32, f32)];

/// Brighter screen in brighter rooms.
pub const SCREEN_CURVE: Curve = &[
    (0.0, 25.0),
    (1.0, 26.0),
    (5.0, 30.0),
    (20.0, 33.0),
    (50.0, 36.0),
    (100.0, 40.0),
    (200.0, 60.0),
    (400.0, 80.0),
    (700.0, 92.0),
    (1000.0, 100.0),
];

/// The other way round: a bright keyboard in the dark, off in daylight.
pub const KBD_CURVE: Curve = &[(0.0, 75.0), (5.0, 50.0), (20.0, 25.0), (50.0, 0.0)];

pub const SCREEN_FLOOR_PCT: f32 = 25.0;
pub const KBD_FLOOR_PCT: f32 = 0.0;
/// Added to the screen percentage while on AC.
pub const AC_SCREEN_BOOST_PCT: f32 = 30.0;

/// Weight of a new lux sample in the low-pass filter. At one sample every
/// 200 ms this is a time constant of about 0.6 s.
pub const LUX_ALPHA: f32 = 0.28;

/// A new target is acted on only when it is this far (percent of
/// max_brightness) from the current one, so lux noise causes no writes.
pub const DEADBAND_PCT: u32 = 2;

/// Every change takes this long, whatever its size.
pub const RAMP: Duration = Duration::from_millis(1000);
/// At most this many writes per change. Each screen write makes the Apple
/// DCP driver sleep 42 ms and run a display commit, so 20 per second is
/// about as fast as it can go anyway.
pub const RAMP_MAX_STEPS: u32 = 20;
/// Smallest step worth a write: 1 % brightness ratio.
const RAMP_LOG_STEP: f32 = 0.01;

/// Automatic control comes back after a manual change once the light has
/// moved this much (percent of the lux at the time of the change) ...
pub const LUX_RESUME_PCT: f32 = 75.0;
/// ... and by at least this many lux, which matters in the dark.
pub const LUX_RESUME_ABS: f32 = 5.0;

pub fn interpolate(curve: Curve, lux: f32) -> f32 {
    let (first, last) = (curve[0], curve[curve.len() - 1]);
    if lux <= first.0 {
        return first.1;
    }
    for w in curve.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if lux <= x1 {
            return y0 + (y1 - y0) * (lux - x0) / (x1 - x0);
        }
    }
    last.1
}

pub struct Settings {
    pub curve: Curve,
    pub floor_pct: f32,
    pub max: u32,
}

/// The brightness the curve asks for, in device units.
pub fn desired(s: &Settings, lux: f32, boost_pct: f32) -> u32 {
    let pct = (interpolate(s.curve, lux) + boost_pct).clamp(s.floor_pct, 100.0);
    (pct * s.max as f32 / 100.0).round() as u32
}

/// Whether `want` is far enough from `target` to start a change.
pub fn worth_moving(target: u32, want: u32, max: u32) -> bool {
    if want == target {
        return false;
    }
    // Always go fully off, or a keyboard could stay faintly lit in daylight.
    if want == 0 {
        return true;
    }
    target.abs_diff(want) >= (max * DEADBAND_PCT / 100).max(1)
}

/// Whether the light has changed enough to end a manual override.
pub fn lux_shifted(lux: f32, baseline: f32) -> bool {
    let d = (lux - baseline).abs();
    d >= LUX_RESUME_ABS && (baseline <= 0.0 || d * 100.0 / baseline >= LUX_RESUME_PCT)
}

/// Low-pass filter over ln(1 + lux). Eyes respond to ratios, so going from
/// 1 to 2 lux counts as much as going from 100 to 200.
#[derive(Default)]
pub struct LuxFilter(Option<f32>);

impl LuxFilter {
    pub fn reset(&mut self) {
        self.0 = None;
    }

    /// Feeds one sample and returns the smoothed lux.
    pub fn update(&mut self, lux: f32) -> f32 {
        let x = lux.max(0.0).ln_1p();
        let y = match self.0 {
            None => x,
            Some(y) => y + LUX_ALPHA * (x - y),
        };
        self.0 = Some(y);
        y.exp_m1()
    }
}

/// One brightness change spread over RAMP. Frames are evenly spaced in time;
/// values are evenly spaced in ln(1 + units), eased in and out.
pub struct Ramp {
    from: f32,
    to: u32,
    start: Instant,
    steps: u32,
    done: u32,
}

impl Ramp {
    pub fn new(from: u32, to: u32, start: Instant) -> Ramp {
        let span = ((to as f32).ln_1p() - (from as f32).ln_1p()).abs();
        let steps = ((span / RAMP_LOG_STEP).ceil() as u32).clamp(1, RAMP_MAX_STEPS);
        Ramp { from: (from as f32).ln_1p(), to, start, steps, done: 0 }
    }

    pub fn steps(&self) -> u32 {
        self.steps
    }

    pub fn finished(&self) -> bool {
        self.done >= self.steps
    }

    /// When the next frame is due.
    pub fn next_at(&self) -> Instant {
        self.start + RAMP * (self.done + 1) / self.steps
    }

    /// Value for the frame due at `now`. Frames missed because a write was
    /// slow are skipped, so the change still takes about RAMP.
    pub fn advance(&mut self, now: Instant) -> u32 {
        let elapsed = now.saturating_duration_since(self.start).as_secs_f32();
        let due = (elapsed / RAMP.as_secs_f32() * self.steps as f32) as u32;
        self.done = due.max(self.done + 1).min(self.steps);
        if self.finished() {
            return self.to;
        }
        let p = self.done as f32 / self.steps as f32;
        let eased = p * p * (3.0 - 2.0 * p);
        let to = (self.to as f32).ln_1p();
        (self.from + (to - self.from) * eased).exp_m1().round() as u32
    }
}

/// One backlight (screen or keyboard).
pub struct Channel {
    pub name: &'static str,
    pub settings: Settings,
    /// What the brightness file holds, as far as we know.
    pub known: u32,
    /// Where the current or last change was heading.
    pub target: u32,
    pub ramp: Option<Ramp>,
    /// Smoothed lux when the user took over; None while automatic.
    pub override_lux: Option<f32>,
}

/// What happened when a sample was applied to a channel.
#[derive(Debug, PartialEq)]
pub enum Update {
    Nothing,
    /// The light moved enough to take over again after a manual change.
    Resumed,
    /// A ramp to this value started.
    Moving(u32),
}

impl Channel {
    pub fn new(name: &'static str, settings: Settings, current: u32) -> Channel {
        Channel { name, settings, known: current, target: current, ramp: None, override_lux: None }
    }

    /// A value read back from the brightness file. Returns true when someone
    /// else changed it; the channel then leaves it alone until the light
    /// shifts (see lux_shifted).
    pub fn observe(&mut self, value: u32, lux: f32) -> bool {
        if value == self.known {
            return false;
        }
        self.known = value;
        self.stop();
        self.override_lux = Some(lux);
        true
    }

    /// Applies a new smoothed lux value.
    pub fn update(&mut self, lux: f32, boost_pct: f32, now: Instant) -> Update {
        let mut result = Update::Nothing;
        if let Some(base) = self.override_lux {
            if !lux_shifted(lux, base) {
                return result;
            }
            self.override_lux = None;
            result = Update::Resumed;
        }
        let want = desired(&self.settings, lux, boost_pct);
        if worth_moving(self.target, want, self.settings.max) {
            self.target = want;
            self.ramp = Some(Ramp::new(self.known, want, now));
            result = Update::Moving(want);
        }
        result
    }

    /// When the next ramp frame is due, if a ramp is running.
    pub fn next_frame(&self) -> Option<Instant> {
        self.ramp.as_ref().map(Ramp::next_at)
    }

    /// The value to write now, if a frame is due and it differs from what
    /// the file already holds.
    pub fn frame(&mut self, now: Instant) -> Option<u32> {
        let ramp = self.ramp.as_mut()?;
        if now < ramp.next_at() {
            return None;
        }
        let value = ramp.advance(now);
        if ramp.finished() {
            self.ramp = None;
        }
        (value != self.known).then_some(value)
    }

    /// Records a successful write (the value read back after it).
    pub fn wrote(&mut self, value: u32) {
        self.known = value;
    }

    /// Ends any ramp where it is, so the next update starts from here.
    pub fn stop(&mut self) {
        self.ramp = None;
        self.target = self.known;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(current: u32) -> Channel {
        Channel::new(
            "screen",
            Settings { curve: SCREEN_CURVE, floor_pct: SCREEN_FLOOR_PCT, max: 500 },
            current,
        )
    }

    /// Runs a channel's ramp to the end and returns the values written.
    fn run_ramp(ch: &mut Channel, start: Instant) -> Vec<u32> {
        let mut written = Vec::new();
        let mut now = start;
        while let Some(at) = ch.next_frame() {
            now = now.max(at);
            if let Some(v) = ch.frame(now) {
                ch.wrote(v);
                written.push(v);
            }
        }
        written
    }

    #[test]
    fn curve_points_and_lines() {
        assert_eq!(interpolate(SCREEN_CURVE, -3.0), 25.0);
        assert_eq!(interpolate(SCREEN_CURVE, 300.0), 70.0);
        assert_eq!(interpolate(SCREEN_CURVE, 5000.0), 100.0);
        assert_eq!(interpolate(KBD_CURVE, 2.5), 62.5);
        assert_eq!(interpolate(KBD_CURVE, 400.0), 0.0);
    }

    #[test]
    fn floor_and_ac_boost() {
        let s = Settings { curve: SCREEN_CURVE, floor_pct: SCREEN_FLOOR_PCT, max: 500 };
        assert_eq!(desired(&s, 0.0, 0.0), 125);
        assert_eq!(desired(&s, 300.0, 0.0), 350);
        assert_eq!(desired(&s, 300.0, AC_SCREEN_BOOST_PCT), 500);
    }

    #[test]
    fn deadband_ignores_noise() {
        // 235 and 240 lux on AC: 465 and 470 of 500, 1 % apart.
        assert!(!worth_moving(465, 470, 500));
        assert!(worth_moving(465, 475, 500));
        assert!(worth_moving(3, 0, 255));
    }

    #[test]
    fn filter_follows_slowly_in_log_space() {
        let mut f = LuxFilter::default();
        assert!((f.update(235.0) - 235.0).abs() < 0.01);
        let y = f.update(240.0);
        assert!(y > 235.0 && y < 237.0, "{y}");
        // A jump from dark to bright is mostly followed within a second.
        let mut f = LuxFilter::default();
        f.update(0.0);
        let mut y = 0.0;
        for _ in 0..5 {
            y = f.update(500.0);
        }
        assert!(y > 100.0, "{y}");
    }

    #[test]
    fn full_swing_takes_at_most_twenty_writes() {
        let t = Instant::now();
        let mut ch = screen(125);
        assert_eq!(ch.update(1000.0, 0.0, t), Update::Moving(500));
        let w = run_ramp(&mut ch, t);
        assert!(w.len() <= RAMP_MAX_STEPS as usize, "{}", w.len());
        assert!(w.windows(2).all(|p| p[0] < p[1]), "{w:?}");
        assert_eq!(*w.last().unwrap(), 500);
        assert_eq!(ch.known, 500);
    }

    #[test]
    fn small_change_takes_few_writes() {
        let t = Instant::now();
        let mut ch = screen(140);
        ch.target = 140;
        ch.ramp = Some(Ramp::new(140, 155, t));
        let w = run_ramp(&mut ch, t);
        assert!(w.len() <= 11, "{w:?}");
        assert_eq!(*w.last().unwrap(), 155);
    }

    #[test]
    fn late_frames_are_skipped() {
        let t = Instant::now();
        let mut r = Ramp::new(125, 500, t);
        assert_eq!(r.steps(), RAMP_MAX_STEPS);
        r.advance(t + RAMP / 2);
        assert!(!r.finished());
        assert_eq!(r.advance(t + RAMP * 2), 500);
        assert!(r.finished());
    }

    #[test]
    fn manual_change_holds_until_light_moves() {
        let t = Instant::now();
        let mut ch = screen(300);
        assert!(!ch.observe(300, 100.0));
        assert!(ch.observe(200, 100.0));
        assert_eq!(ch.update(150.0, 0.0, t), Update::Nothing);
        assert!(ch.ramp.is_none());
        // 100 → 180 lux is +80 %: automatic again, ramping from 200.
        assert_eq!(ch.update(180.0, 0.0, t), Update::Moving(280));
        assert_eq!(ch.override_lux, None);
        assert_eq!(*run_ramp(&mut ch, t).last().unwrap(), 280);
    }

    #[test]
    fn dark_room_needs_five_lux() {
        assert!(!lux_shifted(3.0, 0.0));
        assert!(lux_shifted(5.0, 0.0));
        assert!(!lux_shifted(14.0, 10.0));
        assert!(lux_shifted(18.0, 10.0));
    }
}
