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
/// The Apple panel shows everything up to 2 nits the same: the driver gives
/// 0 to 2 nits its lowest level. 2 nits of a 500-nit maximum is 0.4 %.
pub const SCREEN_DEAD_PCT: f32 = 0.4;
pub const KBD_FLOOR_PCT: f32 = 0.0;
/// Added to the screen percentage while on AC.
pub const AC_SCREEN_BOOST_PCT: f32 = 30.0;

/// Weight of a new lux sample in the low-pass filter. At one sample every
/// 200 ms this is a time constant of about 0.6 s.
pub const LUX_ALPHA: f32 = 0.28;

/// A new target is acted on only when it is this far (percent of
/// max_brightness) from the current one, so lux noise causes no writes.
pub const DEADBAND_PCT: u32 = 2;

/// How fast a channel's brightness may change, in ln(units + DARK_PCT % of
/// the maximum): ratios, since that is what the eye sees, with the dark end
/// compressed (see DARK_PCT).
#[derive(Clone, Copy)]
pub struct Motion {
    /// Top speed, per second.
    pub speed: f32,
    /// How quickly it speeds up and slows down, per second squared.
    pub accel: f32,
}

/// Brightness changes are measured as ratios of (brightness + this much of
/// the maximum). Pure ratios would make the bottom of the range endless:
/// from 1/64 nit to 2 nits is as far as from 4 to 500. But the eye needs
/// bigger steps to notice anything in the dark, and the panel does not even
/// change below 2 nits (the driver gives those its lowest level), so the
/// dark end is compressed. 3 % is 15 nits on a 500-nit screen.
pub const DARK_PCT: f32 = 3.0;

/// The screen moves at most 5.6 % per second: about 0.1 % per write at 60
/// writes a second (the kernel patches), 0.27 % at the 21 the DCP driver
/// takes without them. Around 20 Hz the eye picks up a brightness step of
/// roughly 1 % on a large bright field: 1 % steps did show. Top speed is
/// reached, and given up, over 1.5 s. A small change takes about 1.5 s,
/// 140 to 315 nits about 15 s, 25 % to 100 % about 25 s, and the lowest
/// setting to 25 % about 40 s.
pub const SCREEN_MOTION: Motion = Motion { speed: 0.05625, accel: 0.05625 / 1.5 };
/// Nobody watches the keyboard fade, and from off every unit is a large
/// ratio, so it moves much faster: off to 75 % in about 4 s.
pub const KBD_MOTION: Motion = Motion { speed: 1.0, accel: 1.0 };

/// Brightness keys go from the lowest level to the maximum in this many
/// presses, evenly spaced in the same ratio space as transitions, so every
/// press looks the same size (macOS has 16 steps too).
pub const KEY_STEPS: f32 = 16.0;
/// A press glides there in about 0.65 s, easing in and out over 0.15 s:
/// about 38 writes of at most 0.7 % with the kernel patches (2 % without
/// them already looked smooth enough; 6-7 % looked choppy). Held down, the
/// key sweeps the whole range in about 8 s.
pub const KEY_MOTION: Motion = Motion { speed: 0.4167, accel: 0.4167 / 0.15 };
/// Presses may run at most this many steps ahead of the brightness, so it
/// stops soon after the key is let go.
const KEY_LEAD: f32 = 2.0;

/// Time between writes while a change runs, about 60 a second. With the
/// kernel patches (../kernel) a screen write returns at once and the driver
/// shows it within a frame or two at 120 Hz. Without them each write takes
/// 43 to 47 ms (the Apple DCP driver sleeps 42 ms and commits under the
/// backlight lock), so writes come about 21 a second however short this is.
pub const FRAME: Duration = Duration::from_millis(16);
/// Pause after each write before the next frame looks at the file. A write
/// by someone else that waited for the lock gets it in that time, so the
/// next frame sees it instead of overwriting it.
pub const WRITE_GAP: Duration = Duration::from_millis(1);

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
    pub motion: Motion,
    /// Values up to this one all look the same, so a change starting below
    /// it starts here instead (0 if there is no such range).
    pub dead_zone: u32,
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

/// A running change: where the brightness is and how fast it moves, in
/// ln(units + dark offset), heading for `to` within the channel's Motion
/// limits. It
/// speeds up and slows down gently and never overshoots. A new target only
/// changes where it heads: the speed carries on, and a change that has to
/// turn around slows down first.
///
/// The brightness file takes whole units, so each write is the nearest one
/// to where the motion is. With the kernel patches (1/64 nit) that is fine
/// enough not to show; with whole nits, slow parts move in 1-nit steps.
pub struct Ramp {
    pub to: u32,
    pub motion: Motion,
    /// Started by brightness keys rather than by the light.
    pub by_key: bool,
    /// The dark offset in units (DARK_PCT of the maximum).
    off: f32,
    pos: f32,
    speed: f32,
    last: Instant,
    next_at: Instant,
}

/// The dark offset of a channel in units: DARK_PCT of its maximum.
fn dark_offset(s: &Settings) -> f32 {
    (s.max as f32 * DARK_PCT / 100.0).max(1.0)
}

impl Ramp {
    pub fn new(from: u32, to: u32, s: &Settings, motion: Motion, now: Instant) -> Ramp {
        let off = dark_offset(s);
        // Below the dead zone nothing shows; start rising at its top.
        let from = if from < s.dead_zone && to > s.dead_zone { s.dead_zone } else { from };
        let pos = (from as f32 + off).ln();
        Ramp { to, motion, by_key: false, off, pos, speed: 0.0, last: now, next_at: now }
    }

    /// Current speed, in ln units per second.
    #[cfg(test)]
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// Moves on to `now`. Returns the whole value there, and whether the
    /// target has been reached.
    pub fn advance(&mut self, now: Instant) -> (u32, bool) {
        let m = self.motion;
        // A long gap (a suspend, say) must not turn into one big step.
        let dt = now.saturating_duration_since(self.last).as_secs_f32().min(0.25);
        self.last = now;
        let target = (self.to as f32 + self.off).ln();
        let dist = target - self.pos;
        let dir = dist.signum();
        // As fast as allowed, but no faster than it can still stop in time.
        let want = dir * m.speed.min((2.0 * m.accel * dist.abs()).sqrt());
        self.speed += (want - self.speed).clamp(-m.accel * dt, m.accel * dt);
        let pos = self.pos + self.speed * dt;
        if dist == 0.0 || (target - pos) * dir <= 0.0 {
            self.pos = target;
            self.speed = 0.0;
            return (self.to, true);
        }
        self.pos = pos;
        ((pos.exp() - self.off).round().max(0.0) as u32, false)
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
    /// A change toward this value started or was redirected.
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
            match &mut self.ramp {
                // Only where it heads changes; position, speed and beat stay.
                Some(ramp) => {
                    ramp.to = want;
                    ramp.motion = self.settings.motion;
                    ramp.by_key = false;
                }
                None => {
                    let ramp = Ramp::new(self.known, want, &self.settings, self.settings.motion, now);
                    self.ramp = Some(ramp);
                }
            }
            result = Update::Moving(want);
        }
        result
    }

    /// When the next write is due, if a change is running.
    pub fn next_frame(&self) -> Option<Instant> {
        self.ramp.as_ref().map(|r| r.next_at)
    }

    /// The value to write now, if a write is due and the change has moved
    /// away from what the file holds.
    pub fn frame(&mut self, now: Instant) -> Option<u32> {
        let ramp = self.ramp.as_mut()?;
        if now < ramp.next_at {
            return None;
        }
        let (value, arrived) = ramp.advance(now);
        if arrived {
            self.ramp = None;
        } else {
            ramp.next_at = now + FRAME;
        }
        (value != self.known).then_some(value)
    }

    /// Brightness keys: `presses` steps up (+) or down (-). A step is
    /// 1/KEY_STEPS of the way from the lowest level that shows (the top of
    /// the dead zone) to the maximum, in the ratio space of transitions, so
    /// each press looks the same size. The brightness glides there quickly
    /// (KEY_MOTION), and, as after any manual change, the light is left
    /// alone until it changes a lot. Returns the new target.
    pub fn key_step(&mut self, presses: i32, lux: f32, now: Instant) -> u32 {
        let off = dark_offset(&self.settings);
        let lowest = self.settings.dead_zone.max(1);
        let (lo, hi) = ((lowest as f32 + off).ln(), (self.settings.max as f32 + off).ln());
        let step = (hi - lo) / KEY_STEPS;
        let (pos, base) = match &self.ramp {
            // Presses during a key glide add up from where it heads.
            Some(r) if r.by_key => (r.pos, (r.to as f32 + off).ln()),
            Some(r) => (r.pos, r.pos),
            None => {
                let x = (self.known.max(lowest) as f32 + off).ln();
                (x, x)
            }
        };
        let want = (base + presses as f32 * step)
            .clamp(pos - KEY_LEAD * step, pos + KEY_LEAD * step)
            .clamp(lo, hi);
        let to = if want >= hi - 1e-4 {
            self.settings.max
        } else {
            ((want.exp() - off).round() as u32).clamp(lowest, self.settings.max)
        };
        self.target = to;
        self.override_lux = Some(lux);
        let ramp = self.ramp.get_or_insert_with(|| Ramp::new(self.known, to, &self.settings, KEY_MOTION, now));
        ramp.to = to;
        ramp.motion = KEY_MOTION;
        ramp.by_key = true;
        to
    }

    /// Records a successful write of `value`. Read the file back afterwards
    /// and pass that to observe(): a write by someone else that was waiting
    /// for the lock can land between the two.
    pub fn wrote(&mut self, value: u32) {
        self.known = value;
    }

    /// Delays the next write until at least `t`.
    pub fn not_before(&mut self, t: Instant) {
        if let Some(ramp) = &mut self.ramp {
            ramp.next_at = ramp.next_at.max(t);
        }
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

    /// A screen write with the kernel patches: it returns at once.
    const SCREEN_WRITE: Duration = Duration::from_millis(1);
    /// A screen write without them, as measured on the M1 Pro.
    const BLOCKING_WRITE: Duration = Duration::from_millis(46);

    /// Screen units with the kernel patches: 1/64 nit, 32000 at 500 nits.
    fn nits(n: u32) -> u32 {
        n * 64
    }

    fn screen(current: u32) -> Channel {
        Channel::new(
            "screen",
            Settings {
                curve: SCREEN_CURVE,
                floor_pct: SCREEN_FLOOR_PCT,
                max: nits(500),
                motion: SCREEN_MOTION,
                dead_zone: nits(2),
            },
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

    /// Each written step in the motion space (with the dark offset), in
    /// percent: what the motion limits.
    fn motion_pcts(from: u32, writes: &[(f32, u32)]) -> Vec<f32> {
        let off = nits(500) as f32 * DARK_PCT / 100.0;
        std::iter::once(from)
            .chain(writes.iter().map(|w| w.1))
            .collect::<Vec<_>>()
            .windows(2)
            .map(|p| ((p[1] as f32 + off) / (p[0] as f32 + off)).ln().abs() * 100.0)
            .collect()
    }

    fn start_change(ch: &mut Channel, to: u32, t: Instant) {
        ch.target = to;
        ch.ramp = Some(Ramp::new(ch.known, to, &ch.settings, ch.settings.motion, t));
    }

    /// Runs frames as the daemon would until `until`; returns the writes.
    fn run_until(ch: &mut Channel, start: Instant, until: Instant) -> Vec<(f32, u32)> {
        let mut out = Vec::new();
        while let Some(at) = ch.next_frame().filter(|&at| at < until) {
            if let Some(v) = ch.frame(at) {
                ch.wrote(v);
                ch.not_before(at + SCREEN_WRITE + WRITE_GAP);
                out.push(((at - start).as_secs_f32(), v));
            }
        }
        out
    }

    /// Runs frames until one writes, and returns (when, value).
    fn first_write(ch: &mut Channel) -> (Instant, u32) {
        loop {
            let at = ch.next_frame().unwrap();
            if let Some(v) = ch.frame(at) {
                return (at, v);
            }
        }
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
        let s = Settings {
            curve: SCREEN_CURVE,
            floor_pct: SCREEN_FLOOR_PCT,
            max: 500,
            motion: SCREEN_MOTION,
            dead_zone: 2,
        };
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

    /// Largest written step allowed on the screen, in percent: 0.1 % plus
    /// rounding to whole 1/64-nit units (0.0125 % at 125 nits).
    const MAX_PCT: f32 = 0.12;

    #[test]
    fn screen_steps_stay_under_0_1_percent() {
        let t = Instant::now();
        let mut ch = screen(nits(140));
        start_change(&mut ch, nits(315), t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        let (last_t, last_v) = *w.last().unwrap();
        assert_eq!(last_v, nits(315));
        assert!(last_t > 12.0 && last_t < 17.0, "{last_t}");
        assert!(w.windows(2).all(|p| p[0].1 < p[1].1));
        let pcts = step_pcts(nits(140), &w);
        assert!(pcts.iter().all(|&p| p < MAX_PCT), "{pcts:?}");
        // It eases in and out.
        assert!(pcts[0] < 0.05 && pcts[pcts.len() - 1] < 0.05, "{pcts:?}");
        assert!(ch.ramp.is_none());
    }

    #[test]
    fn a_full_swing_takes_about_24_seconds() {
        let t = Instant::now();
        let mut ch = screen(nits(125));
        assert_eq!(ch.update(1000.0, 0.0, t), Update::Moving(nits(500)));
        let w = run(&mut ch, t, SCREEN_WRITE);
        let last_t = w.last().unwrap().0;
        assert_eq!(w.last().unwrap().1, nits(500));
        assert!(last_t > 20.0 && last_t < 27.0, "{last_t}");
        assert!(step_pcts(nits(125), &w).iter().all(|&p| p < MAX_PCT));
        // Falls are the same the other way.
        let mut ch = screen(nits(500));
        start_change(&mut ch, nits(125), t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(w.last().unwrap().1, nits(125));
        assert!(w.windows(2).all(|p| p[0].1 > p[1].1));
        assert!(step_pcts(nits(500), &w).iter().all(|&p| p < MAX_PCT));
    }

    #[test]
    fn climbing_out_of_the_lowest_setting_shows_soon() {
        // Up to 2 nits the panel does not change: the change starts at
        // 2 nits (an invisible jump) and rises from the first moments.
        let t = Instant::now();
        let mut ch = screen(1);
        start_change(&mut ch, nits(125), t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(w[0], (0.0, nits(2)));
        let visible = w.iter().find(|x| x.1 > nits(2)).unwrap().0;
        assert!(visible < 0.6, "{visible}");
        let (last_t, last_v) = *w.last().unwrap();
        assert_eq!(last_v, nits(125));
        assert!(last_t < 45.0, "{last_t}");
        // Steps shrink as it gets brighter: 1/64 nit is 0.16 % at 10 nits,
        // where the eye needs a few percent; above 100 nits they are as
        // small as anywhere.
        let above = |n: u32| -> Vec<(f32, u32)> { w.iter().copied().filter(|x| x.1 > nits(n)).collect() };
        let (b10, b100) = (above(10), above(100));
        assert!(step_pcts(b10[0].1, &b10[1..]).iter().all(|&p| p < 0.4));
        assert!(step_pcts(b100[0].1, &b100[1..]).iter().all(|&p| p < 0.13));
    }

    #[test]
    fn a_small_change_is_quick() {
        let t = Instant::now();
        let mut ch = screen(nits(300));
        start_change(&mut ch, nits(306), t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(w.last().unwrap().1, nits(306));
        assert!(w.last().unwrap().0 < 2.0, "{w:?}");
    }

    #[test]
    fn a_new_target_keeps_the_speed() {
        let t = Instant::now();
        let mut ch = screen(nits(140));
        ch.update(1000.0, 0.0, t);
        let mid = t + Duration::from_secs(5);
        let before = run_until(&mut ch, t, mid);
        let speed = ch.ramp.as_ref().unwrap().speed();
        let due = ch.next_frame();
        assert_eq!(ch.update(300.0, 0.0, mid), Update::Moving(nits(350)));
        assert_eq!(ch.ramp.as_ref().unwrap().speed(), speed);
        assert_eq!(ch.next_frame(), due);
        let after = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(after.last().unwrap().1, nits(350));
        let all: Vec<_> = before.into_iter().chain(after).collect();
        assert!(step_pcts(nits(140), &all).iter().all(|&p| p < MAX_PCT));
    }

    #[test]
    fn turning_around_slows_down_first() {
        let t = Instant::now();
        let mut ch = screen(nits(140));
        start_change(&mut ch, nits(400), t);
        let mid = t + Duration::from_secs(6);
        let before = run_until(&mut ch, t, mid);
        let top = before.last().unwrap().1;
        start_target(&mut ch, nits(150));
        let after = run(&mut ch, t, SCREEN_WRITE);
        // It goes on up a little, then comes back down, in small steps.
        let peak = after.iter().map(|w| w.1).max().unwrap();
        assert!(peak > top, "{top} {peak}");
        assert_eq!(after.last().unwrap().1, nits(150));
        let all: Vec<_> = before.into_iter().chain(after).collect();
        assert!(step_pcts(nits(140), &all).iter().all(|&p| p < MAX_PCT));
    }

    #[test]
    fn it_never_overshoots() {
        let t = Instant::now();
        let mut ch = screen(nits(140));
        start_change(&mut ch, nits(400), t);
        let mid = t + Duration::from_secs(6);
        let before = run_until(&mut ch, t, mid);
        // At full speed, now asked to stop just ahead.
        let near = before.last().unwrap().1 + nits(1);
        start_target(&mut ch, near);
        let after = run(&mut ch, t, SCREEN_WRITE);
        assert!(after.iter().all(|w| w.1 <= near), "{after:?}");
        assert_eq!(after.last().unwrap().1, near);
    }

    /// Points a running change somewhere else, as update() does.
    fn start_target(ch: &mut Channel, to: u32) {
        ch.target = to;
        ch.ramp.as_mut().unwrap().to = to;
    }

    /// Screen value after `n` presses up from the lowest level.
    fn key_level(n: i32) -> u32 {
        let t = Instant::now();
        let mut ch = screen(nits(2));
        for _ in 0..n {
            ch.key_step(1, 10.0, t);
            run(&mut ch, t, SCREEN_WRITE);
        }
        ch.known
    }

    #[test]
    fn sixteen_presses_span_the_range_in_even_ratios() {
        assert_eq!(key_level(0), nits(2));
        assert_eq!(key_level(16), nits(500));
        let levels: Vec<u32> = (0..=16).map(key_level).collect();
        let off = nits(500) as f32 * DARK_PCT / 100.0;
        let gaps: Vec<f32> =
            levels.windows(2).map(|p| ((p[1] as f32 + off) / (p[0] as f32 + off)).ln()).collect();
        let (lo, hi) = (gaps.iter().cloned().fold(f32::MAX, f32::min), gaps.iter().cloned().fold(0.0, f32::max));
        // Even to within 1.5 % (rounding to whole units near the bottom).
        assert!(hi - lo < 0.004, "{gaps:?}");
        // In plain ratios the presses grow from x3 near the bottom to x1.25
        // at the top, as the eye needs bigger steps in the dark.
        assert!(levels[16] as f32 / levels[15] as f32 > 1.2, "{levels:?}");
    }

    #[test]
    fn a_press_glides_in_small_steps() {
        let t = Instant::now();
        let mut ch = screen(nits(300));
        let to = ch.key_step(-1, 10.0, t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(w.last().unwrap().1, to);
        let last_t = w.last().unwrap().0;
        assert!(last_t > 0.5 && last_t < 0.8, "{last_t} {w:?}");
        assert!(w.len() >= 30, "{w:?}");
        assert!(step_pcts(nits(300), &w).iter().all(|&p| p < 0.8), "{w:?}");
        assert!(w.windows(2).all(|p| p[0].1 > p[1].1));
        // A manual change: the light is left alone until it changes a lot.
        assert_eq!(ch.update(12.0, 30.0, t), Update::Nothing);
        assert_eq!(ch.update(500.0, 30.0, t), Update::Moving(nits(500)));
    }

    #[test]
    fn holding_a_key_moves_steadily_and_stops_soon_after() {
        // mediakeyd repeats every 40 ms after 660 ms; hold for 3 s.
        let t = Instant::now();
        let mut ch = screen(nits(20));
        let mut presses = vec![t];
        let mut at = t + Duration::from_millis(660);
        while at < t + Duration::from_millis(3000) {
            presses.push(at);
            at += Duration::from_millis(40);
        }
        let mut writes = Vec::new();
        for &p in &presses {
            writes.extend(run_until(&mut ch, t, p));
            ch.key_step(1, 10.0, p);
        }
        let release = (*presses.last().unwrap() - t).as_secs_f32();
        writes.extend(run(&mut ch, t, SCREEN_WRITE));
        let end = writes.last().unwrap().0;
        assert!(end - release < 1.2, "{end} {release}");
        assert!(writes.windows(2).all(|p| p[0].1 < p[1].1));
        // 0.7 % a write in the motion space (more in plain ratios at 20
        // nits, where the eye needs it).
        assert!(motion_pcts(nits(20), &writes).iter().all(|&p| p < 0.75), "{writes:?}");
        // About 0.42 ln units a second: well up, but not at the top yet.
        assert!(ch.known > nits(100) && ch.known < nits(500), "{}", ch.known);
    }

    #[test]
    fn a_press_at_the_bottom_stays_there() {
        let t = Instant::now();
        let mut ch = screen(nits(2));
        assert_eq!(ch.key_step(-1, 10.0, t), nits(2));
        assert!(run(&mut ch, t, SCREEN_WRITE).is_empty());
        let mut ch = screen(1);
        assert_eq!(ch.key_step(-3, 10.0, t), nits(2));
    }

    #[test]
    fn without_the_kernel_patches_a_change_takes_as_long() {
        // Writes then block for 46 ms, so there are a third as many, each
        // three times the size, as small as before the patches.
        let t = Instant::now();
        let mut ch = screen(nits(140));
        start_change(&mut ch, nits(315), t);
        let fast = run(&mut ch, t, SCREEN_WRITE);
        let mut ch = screen(nits(140));
        start_change(&mut ch, nits(315), t);
        let slow = run(&mut ch, t, BLOCKING_WRITE);
        let (fast_t, slow_t) = (fast.last().unwrap().0, slow.last().unwrap().0);
        assert!((fast_t - slow_t).abs() < 0.2, "{fast_t} {slow_t}");
        assert!(slow.len() * 5 / 2 < fast.len(), "{} {}", slow.len(), fast.len());
        assert!(step_pcts(nits(140), &slow).iter().all(|&p| p < 0.36));
    }

    #[test]
    fn writes_wait_for_the_previous_one() {
        let t = Instant::now();
        let mut ch = screen(nits(300));
        start_change(&mut ch, nits(150), t);
        let (at, v) = first_write(&mut ch);
        ch.wrote(v);
        let after = at + BLOCKING_WRITE + WRITE_GAP;
        ch.not_before(after);
        assert!(ch.next_frame().unwrap() >= after);
        assert_eq!(ch.frame(after - Duration::from_millis(1)), None);
    }

    #[test]
    fn the_keyboard_comes_on_in_a_few_seconds() {
        // LED writes are instant; they still come at most once a frame.
        let t = Instant::now();
        let mut kbd = Channel::new(
            "kbd",
            Settings { curve: KBD_CURVE, floor_pct: KBD_FLOOR_PCT, max: 255, motion: KBD_MOTION, dead_zone: 0 },
            0,
        );
        assert_eq!(kbd.update(0.0, 0.0, t), Update::Moving(191));
        let w = run(&mut kbd, t, Duration::ZERO);
        assert_eq!(w.last().unwrap().1, 191);
        let last_t = w.last().unwrap().0;
        assert!(last_t > 3.0 && last_t < 6.0, "{last_t}");
        let frame = FRAME.as_secs_f32() - 1e-4;
        assert!(w.windows(2).all(|p| p[1].0 - p[0].0 >= frame), "{w:?}");
    }

    #[test]
    fn a_write_landing_after_ours_is_a_manual_change() {
        // Seen on the M1 Pro: we write 225, a key press that waited for the
        // lock writes 200, and our read-back finds 200.
        let t = Instant::now();
        let mut ch = screen(nits(220));
        start_change(&mut ch, nits(304), t);
        let (at, ours) = first_write(&mut ch);
        ch.wrote(ours);
        assert!(ch.observe(nits(200), 9.0));
        assert!(ch.ramp.is_none());
        assert_eq!((ch.known, ch.target), (nits(200), nits(200)));
        assert_eq!(ch.update(9.0, 30.0, at), Update::Nothing);
    }

    #[test]
    fn manual_change_holds_until_light_moves() {
        let t = Instant::now();
        let mut ch = screen(nits(300));
        assert!(!ch.observe(nits(300), 100.0));
        assert!(ch.observe(nits(200), 100.0));
        assert_eq!(ch.update(150.0, 0.0, t), Update::Nothing);
        assert!(ch.ramp.is_none());
        // 100 → 180 lux is +80 %: automatic again, from 200 nits to 280.
        assert_eq!(ch.update(180.0, 0.0, t), Update::Moving(nits(280)));
        assert_eq!(ch.override_lux, None);
        assert_eq!(run(&mut ch, t, SCREEN_WRITE).last().unwrap().1, nits(280));
    }

    #[test]
    fn dark_room_needs_five_lux() {
        assert!(!lux_shifted(3.0, 0.0));
        assert!(lux_shifted(5.0, 0.0));
        assert!(!lux_shifted(14.0, 10.0));
        assert!(lux_shifted(18.0, 10.0));
    }
}
