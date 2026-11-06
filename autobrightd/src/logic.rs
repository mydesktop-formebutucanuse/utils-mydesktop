// SPDX-License-Identifier: GPL-3.0-or-later
//! Brightness logic without any I/O: curves, lux smoothing, the dead band,
//! transitions and the manual-override rule.
//!
//! The curve points and the override rule come from asahi-brightnessd
//! (Craig Hughes) and juicecultus/asahi-auto-brightness. Their MIT notice,
//! which has to stay with copies of that work, is in NOTICE.

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
pub const RAMP: Duration = Duration::from_secs(6);
/// Writes are at least this far apart. A screen write takes longer anyway:
/// the Apple DCP driver sleeps 42 ms and runs a display commit under the
/// backlight lock (43 to 47 ms measured). This keeps keyboard writes, which
/// are instant, from running at hundreds a second.
pub const MIN_FRAME: Duration = Duration::from_millis(20);
/// Pause after each write before the next frame looks at the file. A write
/// by someone else that waited for the lock gets it in that time, so the
/// next frame sees it instead of overwriting it.
pub const WRITE_GAP: Duration = Duration::from_millis(1);
/// How finely the curve is searched for the moment it reaches the next
/// whole value.
const SCAN: Duration = Duration::from_millis(2);

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

/// A change: a cubic curve in ln(1 + brightness) over RAMP, from where the
/// brightness is to the target, arriving at rest. A fresh change starts at
/// rest too, so it is an S-curve that starts and ends gently. A change
/// redirected midway starts with the speed it had, so it does not jerk.
///
/// The brightness file only takes whole units (nits for the screen), so the
/// curve is not sampled on a fixed beat: each new whole value is written at
/// the moment the curve reaches it. In the dark that is one unit at a time,
/// evenly paced; where the curve moves faster than writes can follow, each
/// write catches up to where the curve is.
pub struct Ramp {
    pub to: u32,
    start: Instant,
    x0: f32,
    /// Starting speed, in ln units per RAMP (a Hermite tangent).
    m0: f32,
    x1: f32,
    next_at: Instant,
}

impl Ramp {
    /// `from` is in ln(1 + units), `speed` in ln units per second.
    pub fn new(from: f32, speed: f32, to: u32, start: Instant) -> Ramp {
        let x1 = (to as f32).ln_1p();
        let span = x1 - from;
        // Keep the speed only if it goes the new way, and only up to three
        // times the span: more would overshoot the target and come back.
        let m0 = if speed * span > 0.0 {
            let limit = 3.0 * span.abs();
            (speed * RAMP.as_secs_f32()).clamp(-limit, limit)
        } else {
            0.0
        };
        Ramp { to, start, x0: from, m0, x1, next_at: start }
    }

    fn progress(&self, t: Instant) -> f32 {
        (t.saturating_duration_since(self.start).as_secs_f32() / RAMP.as_secs_f32()).min(1.0)
    }

    pub fn end(&self) -> Instant {
        self.start + RAMP
    }

    /// Where the curve is at `t`, in ln(1 + units).
    pub fn position(&self, t: Instant) -> f32 {
        let s = self.progress(t);
        let (s2, s3) = (s * s, s * s * s);
        (2.0 * s3 - 3.0 * s2 + 1.0) * self.x0 + (s3 - 2.0 * s2 + s) * self.m0 + (3.0 * s2 - 2.0 * s3) * self.x1
    }

    /// How fast the curve moves at `t`, in ln units per second.
    pub fn speed(&self, t: Instant) -> f32 {
        let s = self.progress(t);
        if s >= 1.0 {
            return 0.0;
        }
        let d = (6.0 * s * s - 6.0 * s) * self.x0
            + (3.0 * s * s - 4.0 * s + 1.0) * self.m0
            + (6.0 * s - 6.0 * s * s) * self.x1;
        d / RAMP.as_secs_f32()
    }

    /// The whole value the curve stands at, at `t`.
    pub fn value(&self, t: Instant) -> u32 {
        if t >= self.end() {
            return self.to;
        }
        self.position(t).exp_m1().round().max(0.0) as u32
    }

    /// The first moment after `after` when the curve reaches a whole value
    /// other than `current`, or the end of the change.
    pub fn next_change(&self, current: u32, after: Instant) -> Instant {
        let mut t = after;
        while t < self.end() {
            t += SCAN;
            if self.value(t) != current {
                return t.min(self.end());
            }
        }
        self.end()
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
    /// No write before this (the previous one and the gap after it).
    quiet_until: Instant,
}

/// What happened when a sample was applied to a channel.
#[derive(Debug, PartialEq)]
pub enum Update {
    Nothing,
    /// The light moved enough to take over again after a manual change.
    Resumed,
    /// A change toward this value started or was redirected.
    Moving(u32),
}

impl Channel {
    pub fn new(name: &'static str, settings: Settings, current: u32) -> Channel {
        Channel {
            name,
            settings,
            known: current,
            target: current,
            ramp: None,
            override_lux: None,
            quiet_until: Instant::now(),
        }
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
            // Carry on from where a running change is, at its speed.
            let (from, speed) = match &self.ramp {
                Some(r) => (r.position(now), r.speed(now)),
                None => ((self.known as f32).ln_1p(), 0.0),
            };
            let mut ramp = Ramp::new(from, speed, want, now);
            ramp.next_at = ramp.next_change(self.known, now);
            self.ramp = Some(ramp);
            result = Update::Moving(want);
        }
        result
    }

    /// When the next write is due, if a change is running.
    pub fn next_frame(&self) -> Option<Instant> {
        self.ramp.as_ref().map(|r| r.next_at.max(self.quiet_until))
    }

    /// The value to write now, if a write is due and the curve has reached a
    /// value the file does not hold yet.
    pub fn frame(&mut self, now: Instant) -> Option<u32> {
        if now < self.next_frame()? {
            return None;
        }
        let ramp = self.ramp.as_mut()?;
        let value = ramp.value(now);
        if now >= ramp.end() {
            self.ramp = None;
        } else {
            ramp.next_at = ramp.next_change(value, now);
        }
        self.quiet_until = now + MIN_FRAME;
        (value != self.known).then_some(value)
    }

    /// Records a successful write of `value`. Read the file back afterwards
    /// and pass that to observe(): a write by someone else that was waiting
    /// for the lock can land between the two.
    pub fn wrote(&mut self, value: u32) {
        self.known = value;
    }

    /// Delays the next write until at least `t`.
    pub fn not_before(&mut self, t: Instant) {
        self.quiet_until = self.quiet_until.max(t);
    }

    /// Ends any change where it is, so the next update starts from here.
    pub fn stop(&mut self) {
        self.ramp = None;
        self.target = self.known;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A screen write as measured on the M1 Pro.
    const SCREEN_WRITE: Duration = Duration::from_millis(46);

    fn screen(current: u32) -> Channel {
        Channel::new(
            "screen",
            Settings { curve: SCREEN_CURVE, floor_pct: SCREEN_FLOOR_PCT, max: 500 },
            current,
        )
    }

    /// Runs a channel's change to the end as the daemon would, each write
    /// taking `write`. Returns (seconds from `start`, value) of each write.
    fn run(ch: &mut Channel, start: Instant, write: Duration) -> Vec<(f32, u32)> {
        let mut out = Vec::new();
        while let Some(at) = ch.next_frame() {
            if let Some(v) = ch.frame(at) {
                ch.wrote(v);
                ch.not_before(at + write + WRITE_GAP);
                out.push(((at - start).as_secs_f32(), v));
            }
        }
        out
    }

    /// Brightness ratio of each written step, in percent.
    fn step_pcts(from: u32, writes: &[(f32, u32)]) -> Vec<f32> {
        std::iter::once(from)
            .chain(writes.iter().map(|w| w.1))
            .collect::<Vec<_>>()
            .windows(2)
            .map(|p| ((p[1] as f32).ln_1p() - (p[0] as f32).ln_1p()).abs() * 100.0)
            .collect()
    }

    fn start_change(ch: &mut Channel, to: u32, t: Instant) {
        let mut ramp = Ramp::new((ch.known as f32).ln_1p(), 0.0, to, t);
        ramp.next_at = ramp.next_change(ch.known, t);
        ch.target = to;
        ch.ramp = Some(ramp);
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
    fn a_change_takes_six_seconds_and_starts_gently() {
        let t = Instant::now();
        let mut ch = screen(140);
        start_change(&mut ch, 301, t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        let (last_t, last_v) = *w.last().unwrap();
        assert_eq!(last_v, 301);
        assert!(last_t > 5.0 && last_t <= 6.0, "{last_t}");
        assert!(w.windows(2).all(|p| p[0].1 < p[1].1));
        // No write right away: the first unit is reached after about 0.2 s,
        // and the first second goes one unit at a time.
        assert!(w[0].0 > 0.15, "{w:?}");
        let first_second: Vec<u32> = w.iter().take_while(|x| x.0 < 1.0).map(|x| x.1).collect();
        assert!(first_second.windows(2).all(|p| p[1] - p[0] == 1), "{first_second:?}");
        assert!(step_pcts(140, &w).iter().all(|&p| p < 1.6), "{w:?}");
        assert!(ch.ramp.is_none());
    }

    #[test]
    fn a_full_swing_stays_under_two_percent_a_step() {
        let t = Instant::now();
        let mut ch = screen(125);
        assert_eq!(ch.update(1000.0, 0.0, t), Update::Moving(500));
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(w.last().unwrap().1, 500);
        assert!(step_pcts(125, &w).iter().all(|&p| p < 2.0));
        // Falls are the same curve the other way.
        let mut ch = screen(500);
        start_change(&mut ch, 125, t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(w.last().unwrap().1, 125);
        assert!(w.windows(2).all(|p| p[0].1 > p[1].1));
        assert!(step_pcts(500, &w).iter().all(|&p| p < 2.0));
    }

    #[test]
    fn a_redirect_keeps_position_and_speed() {
        let t = Instant::now();
        let first = Ramp::new(141f32.ln(), 0.0, 400, t);
        let mid = t + Duration::from_secs(2);
        let (x, v) = (first.position(mid), first.speed(mid));
        assert!(v > 0.0);
        let second = Ramp::new(x, v, 450, mid);
        assert!((second.position(mid) - x).abs() < 1e-6);
        assert!((second.speed(mid) - v).abs() < 1e-4);
        assert_eq!(second.value(mid + RAMP), 450);
    }

    #[test]
    fn a_redirect_never_overshoots() {
        let t = Instant::now();
        let first = Ramp::new(141f32.ln(), 0.0, 400, t);
        let mid = t + Duration::from_secs(3);
        // Moving fast toward 400, now asked to stop just ahead.
        let near = first.value(mid) + 5;
        let second = Ramp::new(first.position(mid), first.speed(mid), near, mid);
        let mut s = mid;
        while s < second.end() {
            assert!(second.value(s) <= near, "{} > {near}", second.value(s));
            s += Duration::from_millis(10);
        }
        // Asked to go back the other way: it starts from rest.
        let back = Ramp::new(first.position(mid), first.speed(mid), 150, mid);
        assert_eq!(back.speed(mid), 0.0);
    }

    #[test]
    fn a_new_target_redirects_a_running_change() {
        let t = Instant::now();
        let mut ch = screen(125);
        ch.update(1000.0, 0.0, t);
        let mid = t + Duration::from_secs(2);
        let v = ch.ramp.as_ref().unwrap().value(mid);
        ch.update(300.0, 0.0, mid);
        let r = ch.ramp.as_ref().unwrap();
        assert_eq!(r.to, 350);
        assert_eq!(r.value(mid), v);
    }

    #[test]
    fn writes_wait_for_the_previous_one() {
        let t = Instant::now();
        let mut ch = screen(300);
        start_change(&mut ch, 150, t);
        let at = ch.next_frame().unwrap();
        let v = ch.frame(at).unwrap();
        ch.wrote(v);
        let after = at + SCREEN_WRITE + WRITE_GAP;
        ch.not_before(after);
        assert!(ch.next_frame().unwrap() >= after);
        assert_eq!(ch.frame(after - Duration::from_millis(1)), None);
    }

    #[test]
    fn the_keyboard_from_off_is_capped_at_fifty_writes_a_second() {
        let t = Instant::now();
        let mut kbd = Channel::new(
            "kbd",
            Settings { curve: KBD_CURVE, floor_pct: KBD_FLOOR_PCT, max: 255 },
            0,
        );
        assert_eq!(kbd.update(0.0, 0.0, t), Update::Moving(191));
        let w = run(&mut kbd, t, Duration::ZERO);
        assert_eq!(w.last().unwrap().1, 191);
        assert!(w.windows(2).all(|p| p[1].0 - p[0].0 >= 0.0199), "{w:?}");
    }

    #[test]
    fn a_write_landing_after_ours_is_a_manual_change() {
        // Seen on the M1 Pro: we write 225, a key press that waited for the
        // lock writes 200, and our read-back finds 200.
        let t = Instant::now();
        let mut ch = screen(220);
        start_change(&mut ch, 304, t);
        let at = ch.next_frame().unwrap();
        let ours = ch.frame(at).unwrap();
        ch.wrote(ours);
        assert!(ch.observe(200, 9.0));
        assert!(ch.ramp.is_none());
        assert_eq!((ch.known, ch.target), (200, 200));
        assert_eq!(ch.update(9.0, 30.0, at), Update::Nothing);
    }

    #[test]
    fn manual_change_holds_until_light_moves() {
        let t = Instant::now();
        let mut ch = screen(300);
        assert!(!ch.observe(300, 100.0));
        assert!(ch.observe(200, 100.0));
        assert_eq!(ch.update(150.0, 0.0, t), Update::Nothing);
        assert!(ch.ramp.is_none());
        // 100 → 180 lux is +80 %: automatic again, from 200.
        assert_eq!(ch.update(180.0, 0.0, t), Update::Moving(280));
        assert_eq!(ch.override_lux, None);
        assert_eq!(run(&mut ch, t, SCREEN_WRITE).last().unwrap().1, 280);
    }

    #[test]
    fn dark_room_needs_five_lux() {
        assert!(!lux_shifted(3.0, 0.0));
        assert!(lux_shifted(5.0, 0.0));
        assert!(!lux_shifted(14.0, 10.0));
        assert!(lux_shifted(18.0, 10.0));
    }
}
