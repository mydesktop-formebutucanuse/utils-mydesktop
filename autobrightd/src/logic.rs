// SPDX-License-Identifier: GPL-3.0-or-later
//! Brightness logic without any I/O: curves, lux smoothing, the dead band,
//! ramps and the manual-override rule.
//!
//! The curve points, the transition step and the override rule come from
//! asahi-brightnessd (Craig Hughes) and juicecultus/asahi-auto-brightness.
//! Their MIT notice, which has to stay with copies of that work, is in NOTICE.

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

/// Time between frames while a change runs: about 22 a second. Each screen
/// write makes the Apple DCP driver sleep 42 ms and run a display commit
/// (43 to 47 ms measured), under a lock, so this is as fast as it goes.
pub const FRAME: Duration = Duration::from_millis(45);
/// Pause after each write before the next frame looks at the file. A write
/// by someone else that waited for the lock gets it in that time, so the
/// next frame sees it instead of overwriting it.
pub const WRITE_GAP: Duration = Duration::from_millis(1);
/// Each frame moves 1/2^SMOOTH_SHIFT of the remaining gap, and at least one
/// unit: asahi-brightnessd's transition, fast at first and slowing down.
pub const SMOOTH_SHIFT: u32 = 4;

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

/// One frame of a transition: 1/16 of the gap (rounded toward minus
/// infinity, as an arithmetic shift does in the C original), at least 1.
pub fn step_toward(current: u32, target: u32) -> u32 {
    let delta = target as i64 - current as i64;
    let step = match delta >> SMOOTH_SHIFT {
        0 if delta > 0 => 1,
        s if delta < 0 && s > -1 => -1,
        s => s,
    };
    (current as i64 + step) as u32
}

/// A running change: where it goes and when its next frame is due.
pub struct Ramp {
    pub to: u32,
    next_at: Instant,
}

impl Ramp {
    pub fn new(to: u32, now: Instant) -> Ramp {
        Ramp { to, next_at: now }
    }

    pub fn next_at(&self) -> Instant {
        self.next_at
    }

    /// The value for the frame starting at `now`, from what the file holds.
    pub fn advance(&mut self, current: u32, now: Instant) -> u32 {
        self.next_at = now + FRAME;
        step_toward(current, self.to)
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
            // A running change just turns toward the new value.
            match &mut self.ramp {
                Some(ramp) => ramp.to = want,
                None => self.ramp = Some(Ramp::new(want, now)),
            }
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
        let value = ramp.advance(self.known, now);
        if value == ramp.to {
            self.ramp = None;
        }
        (value != self.known).then_some(value)
    }

    /// Records a successful write of `value`. Read the file back afterwards
    /// and pass that to observe(): a write by someone else that was waiting
    /// for the lock can land between the two.
    pub fn wrote(&mut self, value: u32) {
        self.known = value;
    }

    /// Delays the next frame until at least `t`.
    pub fn not_before(&mut self, t: Instant) {
        if let Some(ramp) = &mut self.ramp {
            ramp.next_at = ramp.next_at.max(t);
        }
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
    fn steps_are_a_sixteenth_of_the_gap() {
        assert_eq!(step_toward(125, 500), 148);
        assert_eq!(step_toward(500, 125), 476);
        assert_eq!(step_toward(140, 155), 141);
        assert_eq!(step_toward(155, 140), 154);
        assert_eq!(step_toward(140, 140), 140);
    }

    #[test]
    fn full_swing_follows_the_original_curve() {
        let t = Instant::now();
        let mut ch = screen(125);
        assert_eq!(ch.update(1000.0, 0.0, t), Update::Moving(500));
        let w = run_ramp(&mut ch, t);
        // The same 73 writes as asahi-brightnessd, 45 ms apart: about 3.3 s.
        assert_eq!(w.len(), 73);
        assert!(w.windows(2).all(|p| p[0] < p[1]), "{w:?}");
        assert_eq!(*w.last().unwrap(), 500);
        assert_eq!(ch.known, 500);
        assert!(ch.ramp.is_none());
    }

    #[test]
    fn frames_are_45_ms_apart() {
        let t = Instant::now();
        let mut ch = screen(300);
        ch.ramp = Some(Ramp::new(150, t));
        assert_eq!(ch.frame(t), Some(290));
        ch.wrote(290);
        assert_eq!(ch.frame(t + FRAME / 2), None);
        assert_eq!(ch.next_frame(), Some(t + FRAME));
        assert_eq!(ch.frame(t + FRAME), Some(281));
    }

    #[test]
    fn new_target_turns_a_running_change() {
        let t = Instant::now();
        let mut ch = screen(125);
        ch.update(1000.0, 0.0, t);
        let at = ch.next_frame().unwrap();
        ch.update(300.0, 0.0, t);
        assert_eq!(ch.ramp.as_ref().map(|r| r.to), Some(350));
        assert_eq!(ch.next_frame(), Some(at));
    }

    #[test]
    fn a_write_landing_after_ours_is_a_manual_change() {
        // Seen on the M1 Pro: we write 225, a key press that waited for the
        // lock writes 200, and our read-back finds 200.
        let t = Instant::now();
        let mut ch = screen(220);
        ch.ramp = Some(Ramp::new(304, t));
        assert_eq!(ch.frame(t), Some(225));
        ch.wrote(225);
        assert!(ch.observe(200, 9.0));
        assert!(ch.ramp.is_none());
        assert_eq!((ch.known, ch.target), (200, 200));
        assert_eq!(ch.update(9.0, 30.0, t), Update::Nothing);
    }

    #[test]
    fn the_gap_after_a_write_delays_the_next_frame() {
        let t = Instant::now();
        let mut ch = screen(300);
        ch.ramp = Some(Ramp::new(150, t));
        ch.frame(t);
        let late = t + FRAME + WRITE_GAP;
        ch.not_before(late);
        assert_eq!(ch.next_frame(), Some(late));
        ch.not_before(t);
        assert_eq!(ch.next_frame(), Some(late));
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
