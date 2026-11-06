// SPDX-License-Identifier: GPL-3.0-or-later
//! Brightness logic without any I/O: curves, lux smoothing, the dead band,
//! transitions and the manual-override rule.
//!
//! The curve points and the override rule come from asahi-brightnessd
//! (Craig Hughes) and juicecultus/asahi-auto-brightness. Their MIT notice,
//! which has to stay with copies of that work, is in NOTICE.

use std::rc::Rc;
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

/// From the lowest level that shows to the maximum is ln(1.03 / 0.034) =
/// 3.41 in this space (dead zone 0.4 %, dark offset 3 %), whatever the
/// maximum.
pub const FULL_RANGE: f32 = 3.41;
/// The screen goes from its lowest level to the maximum in about 8 s: top
/// speed 0.47 a second, reached and given up over 0.75 s, about 0.8 % per
/// write at 60 writes a second (2.2 % at the 21 the DCP driver takes
/// without the kernel patches). Every change goes at this speed, so a new
/// target during a change never makes it slow down. 140 to 315 nits takes
/// about 2.4 s, a small change about 0.35 s.
pub const SCREEN_MOTION: Motion = Motion { speed: FULL_RANGE / 7.25, accel: FULL_RANGE / 7.25 / 0.75 };
/// Nobody watches the keyboard fade, and from off every unit is a large
/// ratio, so it moves much faster: off to 75 % in about 2 s.
pub const KBD_MOTION: Motion = Motion { speed: 2.0, accel: 4.0 };

/// Brightness keys go from the lowest level to the maximum in this many
/// presses, evenly spaced in the same ratio space as transitions, so every
/// press looks the same size (macOS has 16 steps too).
pub const KEY_STEPS: f32 = 16.0;
/// A press glides there in about 0.13 s, easing in and out over 0.03 s, in
/// about 8 writes. Held down, the brightness moves on at this speed until
/// the key is let go (the whole range in about 1.6 s), then stops within
/// 0.03 s.
pub const KEY_MOTION: Motion = Motion { speed: 2.083, accel: 2.083 / 0.03 };
/// Presses in a row may run at most this many steps ahead of the brightness.
const KEY_LEAD: f32 = 2.0;
/// A held key that has sent no press or repeat for this long counts as let
/// go, in case its release got lost. Longer than the first repeat delay of
/// the brightness keys (300 ms in mediakeyd's example config).
pub const KEY_HOLD_TIMEOUT: Duration = Duration::from_millis(400);

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

/// How the brightness the panel shows follows the value written. The Apple
/// panel does not follow the driver's nits evenly: measured on an M1 Pro
/// through what the DCP reports (actual_brightness), it rises 1.4 times as
/// fast as the written value just above about 130 and 255 nits, and 0.7
/// times as fast near the top, so an even change of the written value
/// lurches there. `autobrightd --calibrate` measures the panel; changes then
/// move evenly in what the panel shows. Points (written, shown) in device
/// units, both rising, joined by straight lines; outside them, the offset
/// of the nearest point.
#[derive(Debug)]
pub struct PanelMap {
    shown: Vec<(f32, f32)>,
    written: Vec<(f32, f32)>,
}

/// y at `x` on the line through `points` (x, y), x rising.
fn through(points: &[(f32, f32)], x: f32) -> f32 {
    let i = points.partition_point(|p| p.0 < x);
    if i == 0 || i == points.len() {
        let p = points[i.min(points.len() - 1)];
        return x + p.1 - p.0;
    }
    let (a, b) = (points[i - 1], points[i]);
    a.1 + (x - a.0) * (b.1 - a.1) / (b.0 - a.0)
}

impl PanelMap {
    /// From measured (written, shown) points; drops any that do not rise in
    /// both. None if fewer than two are left.
    pub fn new(mut points: Vec<(f32, f32)>) -> Option<PanelMap> {
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut shown: Vec<(f32, f32)> = Vec::new();
        for p in points {
            if shown.last().is_none_or(|l| p.0 > l.0 && p.1 > l.1) {
                shown.push(p);
            }
        }
        let written = shown.iter().map(|&(w, s)| (s, w)).collect();
        (shown.len() >= 2).then_some(PanelMap { shown, written })
    }

    pub fn len(&self) -> usize {
        self.shown.len()
    }

    /// The (written, shown) points.
    pub fn points(&self) -> &[(f32, f32)] {
        &self.shown
    }

    /// What the panel shows for a written value.
    pub fn shown(&self, written: f32) -> f32 {
        through(&self.shown, written)
    }

    /// The value to write for the panel to show `shown`.
    pub fn written(&self, shown: f32) -> f32 {
        through(&self.written, shown)
    }
}

/// What the panel shows for a written value, with or without a map.
fn shown(map: Option<&Rc<PanelMap>>, written: f32) -> f32 {
    map.map_or(written, |m| m.shown(written))
}

/// The value to write for the panel to show `x`, with or without a map.
fn written(map: Option<&Rc<PanelMap>>, x: f32) -> f32 {
    map.map_or(x, |m| m.written(x))
}

/// A running change: where the brightness is and how fast it moves, in
/// ln(shown units + dark offset), heading for `to` within the channel's
/// Motion limits. Shown units are what the panel shows (PanelMap), or the
/// written value without a map. It
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
    map: Option<Rc<PanelMap>>,
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
    pub fn new(from: u32, to: u32, s: &Settings, motion: Motion, map: Option<Rc<PanelMap>>, now: Instant) -> Ramp {
        let off = dark_offset(s);
        // Below the dead zone nothing shows; start rising at its top.
        let from = if from < s.dead_zone && to > s.dead_zone { s.dead_zone } else { from };
        let pos = (shown(map.as_ref(), from as f32) + off).ln();
        Ramp { to, motion, by_key: false, off, map, pos, speed: 0.0, last: now, next_at: now }
    }

    /// A written value as a position (ln units).
    fn at(&self, value: u32) -> f32 {
        (shown(self.map.as_ref(), value as f32) + self.off).ln()
    }

    /// The nearest written value to a position.
    fn value(&self, x: f32) -> u32 {
        written(self.map.as_ref(), x.exp() - self.off).round().max(0.0) as u32
    }

    /// Current speed, in ln units per second.
    #[cfg(test)]
    pub fn speed(&self) -> f32 {
        self.speed
    }

    /// The target in ln units.
    fn target(&self) -> f32 {
        self.at(self.to)
    }

    /// Where it would come to rest if it started slowing down now.
    fn stopping_point(&self) -> f32 {
        self.pos + self.speed * self.speed.abs() / (2.0 * self.motion.accel)
    }

    /// Heads for `x` (ln units), the nearest whole value.
    fn head_for(&mut self, x: f32) {
        self.to = self.value(x);
    }

    /// Moves on to `now`. Returns the whole value there, and whether the
    /// target has been reached.
    pub fn advance(&mut self, now: Instant) -> (u32, bool) {
        let m = self.motion;
        // A long gap (a suspend, say) must not turn into one big step.
        let dt = now.saturating_duration_since(self.last).as_secs_f32().min(0.25);
        self.last = now;
        let target = self.target();
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
        (self.value(pos), false)
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
    /// A brightness key that is down.
    key: Option<KeyHold>,
    /// How the panel follows the written value, if measured.
    pub map: Option<Rc<PanelMap>>,
}

/// A brightness key that is down. The brightness moves on in its direction
/// until the key is let go.
struct KeyHold {
    /// +1 brighter, -1 dimmer.
    dir: f32,
    /// One step from where the press started (ln units). Letting go stops
    /// the brightness at once, but not before this, so a tap is a whole step.
    min_to: f32,
    /// The last press or repeat.
    last: Instant,
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
        Channel { name, settings, known: current, target: current, ramp: None, override_lux: None, key: None, map: None }
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
            self.head_for(want, now);
            result = Update::Moving(want);
        }
        result
    }

    /// Starts or redirects an automatic change toward `to`. Only where it
    /// heads changes; position, speed and beat stay.
    pub fn head_for(&mut self, to: u32, now: Instant) {
        self.target = to;
        self.key = None;
        let (s, map) = (&self.settings, &self.map);
        let ramp = self.ramp.get_or_insert_with(|| Ramp::new(self.known, to, s, s.motion, map.clone(), now));
        ramp.to = to;
        ramp.by_key = false;
        ramp.motion = self.settings.motion;
    }

    /// When the next write is due, if a change is running.
    pub fn next_frame(&self) -> Option<Instant> {
        self.ramp.as_ref().map(|r| r.next_at)
    }

    /// The value to write now, if a write is due and the change has moved
    /// away from what the file holds.
    pub fn frame(&mut self, now: Instant) -> Option<u32> {
        if now < self.ramp.as_ref()?.next_at {
            return None;
        }
        self.keep_key_moving(now);
        let ramp = self.ramp.as_mut()?;
        let (value, arrived) = ramp.advance(now);
        if arrived {
            self.ramp = None;
        } else {
            ramp.next_at = now + FRAME;
        }
        (value != self.known).then_some(value)
    }

    /// The range brightness keys move in, (lowest, highest) in ln units:
    /// from the lowest level that shows (the top of the dead zone) to the
    /// maximum.
    fn key_range(&self) -> (f32, f32) {
        let lowest = self.settings.dead_zone.max(1);
        (self.at(lowest), self.at(self.settings.max))
    }

    /// A written value as a position (ln of shown units + dark offset).
    fn at(&self, value: u32) -> f32 {
        (shown(self.map.as_ref(), value as f32) + dark_offset(&self.settings)).ln()
    }

    /// Brightness keys: `presses` steps up (+) or down (-). A step is
    /// 1/KEY_STEPS of the key range, in the ratio space of transitions, so
    /// each press looks the same size. The brightness glides there quickly
    /// (KEY_MOTION) and, while the key stays down, moves on until it is let
    /// go (key_release). As after any manual change, the light is then left
    /// alone until it changes a lot. A repeat of a key that is down only
    /// shows that it still is. Returns the new target.
    pub fn key_step(&mut self, presses: i32, lux: f32, now: Instant) -> u32 {
        let dir = presses.signum() as f32;
        if let Some(hold) = self.key.as_mut().filter(|h| h.dir == dir) {
            hold.last = now;
            return self.target;
        }
        let lowest = self.settings.dead_zone.max(1);
        let (lo, hi) = self.key_range();
        let step = (hi - lo) / KEY_STEPS;
        let (pos, base) = match &self.ramp {
            // Presses during a key glide add up from where it heads.
            Some(r) if r.by_key => (r.pos, r.target()),
            Some(r) => (r.pos, r.pos),
            None => {
                let x = self.at(self.known.max(lowest));
                (x, x)
            }
        };
        let want = (base + presses as f32 * step)
            .clamp(pos - KEY_LEAD * step, pos + KEY_LEAD * step)
            .clamp(lo, hi);
        let to = if want >= hi - 1e-4 {
            self.settings.max
        } else {
            let x = written(self.map.as_ref(), want.exp() - dark_offset(&self.settings));
            (x.round().max(0.0) as u32).clamp(lowest, self.settings.max)
        };
        self.target = to;
        self.override_lux = Some(lux);
        self.key = Some(KeyHold { dir, min_to: want, last: now });
        let (s, map) = (&self.settings, &self.map);
        let ramp = self.ramp.get_or_insert_with(|| Ramp::new(self.known, to, s, KEY_MOTION, map.clone(), now));
        ramp.to = to;
        ramp.motion = KEY_MOTION;
        ramp.by_key = true;
        to
    }

    /// The brightness key was let go: stop as soon as the motion allows,
    /// but not before the whole step of the press.
    pub fn key_release(&mut self) {
        let Some(hold) = self.key.take() else { return };
        let (lo, hi) = self.key_range();
        let Some(ramp) = self.ramp.as_mut().filter(|r| r.by_key) else { return };
        let stop = ramp.stopping_point();
        let x = if (stop - hold.min_to) * hold.dir >= 0.0 { stop } else { hold.min_to };
        ramp.head_for(x.clamp(lo, hi));
        self.target = ramp.to;
    }

    /// While a key is down, keeps the target just far enough ahead that the
    /// brightness moves on at full speed; a key with no press or repeat for
    /// KEY_HOLD_TIMEOUT counts as let go.
    fn keep_key_moving(&mut self, now: Instant) {
        let Some(hold) = &self.key else { return };
        if now.saturating_duration_since(hold.last) > KEY_HOLD_TIMEOUT {
            self.key_release();
            return;
        }
        let (dir, min_to) = (hold.dir, hold.min_to);
        let (lo, hi) = self.key_range();
        let Some(ramp) = self.ramp.as_mut().filter(|r| r.by_key) else { return };
        // What it needs to stop from full speed, plus a few frames.
        let m = ramp.motion;
        let lead = m.speed * m.speed / (2.0 * m.accel) + m.speed * 0.05;
        let ahead = ramp.pos + dir * lead;
        let x = if (ahead - min_to) * dir >= 0.0 { ahead } else { min_to };
        ramp.head_for(x.clamp(lo, hi));
        self.target = ramp.to;
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
        self.key = None;
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
        ch.ramp = Some(Ramp::new(ch.known, to, &ch.settings, ch.settings.motion, ch.map.clone(), t));
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

    /// Largest written step of an automatic change, in percent: 0.75 % at
    /// top speed in the motion space (writes 16 ms apart here), a little
    /// more in plain ratios (+ 15 nits of dark offset), plus rounding.
    const MAX_PCT: f32 = 1.0;

    #[test]
    fn screen_steps_stay_under_1_percent() {
        let t = Instant::now();
        let mut ch = screen(nits(140));
        start_change(&mut ch, nits(315), t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        let (last_t, last_v) = *w.last().unwrap();
        assert_eq!(last_v, nits(315));
        assert!(last_t > 2.1 && last_t < 2.6, "{last_t}");
        assert!(w.windows(2).all(|p| p[0].1 < p[1].1));
        let pcts = step_pcts(nits(140), &w);
        assert!(pcts.iter().all(|&p| p < MAX_PCT), "{pcts:?}");
        // It eases in and out.
        assert!(pcts[0] < 0.1 && pcts[pcts.len() - 1] < 0.1, "{pcts:?}");
        assert!(ch.ramp.is_none());
    }

    #[test]
    fn the_whole_range_takes_about_8_seconds() {
        let t = Instant::now();
        for (from, to) in [(nits(2), nits(500)), (nits(500), nits(2))] {
            let mut ch = screen(from);
            ch.head_for(to, t);
            let w = run(&mut ch, t, SCREEN_WRITE);
            let last_t = w.last().unwrap().0;
            assert_eq!(w.last().unwrap().1, to);
            assert!(last_t > 7.6 && last_t < 8.4, "{from} {to} {last_t}");
            assert!(motion_pcts(from, &w).iter().all(|&p| p < 0.9), "{from} {to}");
        }
        // A full swing on AC (125 to 500 nits) takes about 3.5 s.
        let mut ch = screen(nits(125));
        assert_eq!(ch.update(1000.0, 0.0, t), Update::Moving(nits(500)));
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert!(w.last().unwrap().0 > 3.2 && w.last().unwrap().0 < 3.8, "{}", w.last().unwrap().0);
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
        assert!(visible < 0.3, "{visible}");
        let (last_t, last_v) = *w.last().unwrap();
        assert_eq!(last_v, nits(125));
        assert!(last_t < 5.5, "{last_t}");
        // Steps shrink as it gets brighter: the eye needs bigger ones in the
        // dark; above 100 nits they are as small as anywhere.
        let above = |n: u32| -> Vec<(f32, u32)> { w.iter().copied().filter(|x| x.1 > nits(n)).collect() };
        let (b10, b100) = (above(10), above(100));
        assert!(step_pcts(b10[0].1, &b10[1..]).iter().all(|&p| p < 2.4));
        assert!(step_pcts(b100[0].1, &b100[1..]).iter().all(|&p| p < 1.05));
    }

    #[test]
    fn a_small_change_is_quick() {
        let t = Instant::now();
        let mut ch = screen(nits(300));
        start_change(&mut ch, nits(306), t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(w.last().unwrap().1, nits(306));
        assert!(w.last().unwrap().0 < 0.5, "{w:?}");
    }

    #[test]
    fn a_new_target_keeps_the_speed() {
        // The light shifts during a change: it heads elsewhere at the
        // speed it has, without slowing down.
        let t = Instant::now();
        let mut ch = screen(nits(140));
        ch.update(1000.0, 0.0, t);
        let mid = t + Duration::from_secs(1);
        let before = run_until(&mut ch, t, mid);
        let speed = ch.ramp.as_ref().unwrap().speed();
        let due = ch.next_frame();
        let to = desired(&ch.settings, 800.0, 0.0);
        assert_eq!(ch.update(800.0, 0.0, mid), Update::Moving(to));
        assert_eq!(ch.ramp.as_ref().unwrap().speed(), speed);
        assert_eq!(ch.ramp.as_ref().unwrap().motion.speed, SCREEN_MOTION.speed);
        assert_eq!(ch.next_frame(), due);
        let after = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(after.last().unwrap().1, to);
        let all: Vec<_> = before.into_iter().chain(after).collect();
        assert!(step_pcts(nits(140), &all).iter().all(|&p| p < MAX_PCT));
        // Full speed on until it slows down for the end.
        let m = motion_pcts(nits(140), &all);
        let mid_run: Vec<f32> = all.iter().zip(&m).filter(|(x, _)| x.0 > 0.8 && x.0 < 1.8).map(|(_, &p)| p).collect();
        assert!(mid_run.iter().all(|&p| p > 0.7), "{mid_run:?}");
    }

    #[test]
    fn turning_around_slows_down_first() {
        let t = Instant::now();
        let mut ch = screen(nits(140));
        start_change(&mut ch, nits(400), t);
        let mid = t + Duration::from_secs(1);
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
        let mid = t + Duration::from_millis(1200);
        let before = run_until(&mut ch, t, mid);
        // At full speed, now asked to stop just ahead.
        let near = before.last().unwrap().1 + nits(1);
        start_target(&mut ch, near);
        let after = run(&mut ch, t, SCREEN_WRITE);
        assert!(after.iter().all(|w| w.1 <= near), "{after:?}");
        assert_eq!(after.last().unwrap().1, near);
    }

    /// A panel like the M1 Pro's: it follows the written value until 250
    /// nits, then rises 1.5 times as fast.
    fn kinked_map() -> Rc<PanelMap> {
        let k = nits(250) as f32;
        let points = (0..=60).map(|i| {
            let w = i as f32 * nits(10) as f32;
            (w, if w <= k { w } else { k + 1.5 * (w - k) })
        });
        Rc::new(PanelMap::new(points.collect()).unwrap())
    }

    #[test]
    fn panel_map_lines_and_inverse() {
        let map = PanelMap::new(vec![(100.0, 120.0), (200.0, 250.0), (150.0, 150.0), (300.0, 240.0)]).unwrap();
        // (300, 240) does not rise in what it shows, so it is dropped.
        assert_eq!(map.points(), &[(100.0, 120.0), (150.0, 150.0), (200.0, 250.0)]);
        assert_eq!(map.shown(125.0), 135.0);
        assert_eq!(map.shown(175.0), 200.0);
        assert_eq!(map.written(200.0), 175.0);
        // Outside the points: the nearest point's offset.
        assert_eq!(map.shown(50.0), 70.0);
        assert_eq!(map.shown(260.0), 310.0);
        for w in [60.0, 101.0, 149.0, 199.5, 400.0] {
            assert!((map.written(map.shown(w)) - w).abs() < 1e-3, "{w}");
        }
        assert!(PanelMap::new(vec![(1.0, 1.0)]).is_none());
    }

    #[test]
    fn with_a_panel_map_changes_are_even_in_what_the_panel_shows() {
        let t = Instant::now();
        let map = kinked_map();
        let mut ch = screen(nits(150));
        ch.map = Some(map.clone());
        ch.head_for(nits(400), t);
        let w = run(&mut ch, t, SCREEN_WRITE);
        assert_eq!(w.last().unwrap().1, nits(400));
        // In what the panel shows, the steps at full speed (0.75 % a write
        // 16 ms apart) are all alike across the kink ...
        let shown: Vec<(f32, u32)> = w.iter().map(|&(t, v)| (t, map.shown(v as f32).round() as u32)).collect();
        let from = map.shown(nits(150) as f32) as u32;
        let even: Vec<f32> = motion_pcts(from, &shown).into_iter().zip(&w).filter(|(_, x)| x.0 > 0.8 && x.0 < w.last().unwrap().0 - 0.8).map(|(p, _)| p).collect();
        let (lo, hi) = (even.iter().cloned().fold(9.0, f32::min), even.iter().cloned().fold(0.0, f32::max));
        assert!(lo > 0.73 && hi < 0.77, "{lo} {hi}");
        // ... while the written values slow down above it.
        let below = w.iter().find(|x| x.1 > nits(200)).unwrap();
        let above = w.iter().find(|x| x.1 > nits(330)).unwrap();
        let rate = |x: &(f32, u32)| {
            let i = w.iter().position(|y| y == x).unwrap();
            ((w[i].1 as f32).ln() - (w[i - 1].1 as f32).ln()) / (w[i].0 - w[i - 1].0)
        };
        assert!(rate(above) < rate(below) * 0.8, "{} {}", rate(below), rate(above));
    }

    #[test]
    fn with_a_panel_map_key_steps_are_even_in_what_the_panel_shows() {
        let t = Instant::now();
        let map = kinked_map();
        let mut ch = screen(nits(2));
        ch.map = Some(map.clone());
        let off = nits(500) as f32 * DARK_PCT / 100.0;
        let mut levels = vec![map.shown(ch.known as f32)];
        for _ in 0..16 {
            tap(&mut ch, 1, t);
            levels.push(map.shown(ch.known as f32));
        }
        assert_eq!(ch.known, nits(500));
        let gaps: Vec<f32> = levels.windows(2).map(|p| ((p[1] + off) / (p[0] + off)).ln()).collect();
        let (lo, hi) = (gaps.iter().cloned().fold(9.0, f32::min), gaps.iter().cloned().fold(0.0, f32::max));
        assert!(hi - lo < 0.004, "{gaps:?}");
    }

    /// Points a running change somewhere else, as update() does.
    fn start_target(ch: &mut Channel, to: u32) {
        ch.target = to;
        ch.ramp.as_mut().unwrap().to = to;
    }

    /// A tap: a press, let go at once. Returns the writes.
    fn tap(ch: &mut Channel, presses: i32, t: Instant) -> Vec<(f32, u32)> {
        ch.key_step(presses, 10.0, t);
        ch.key_release();
        run(ch, t, SCREEN_WRITE)
    }

    /// Holds a key from `t` for `secs` with mediakeyd's repeats (after
    /// 300 ms, every 40 ms), then lets it go if `release`. Returns the
    /// writes to the end of the motion.
    fn hold(ch: &mut Channel, dir: i32, t: Instant, secs: f32, release: bool) -> Vec<(f32, u32)> {
        let end = t + Duration::from_secs_f32(secs);
        ch.key_step(dir, 10.0, t);
        let mut writes = Vec::new();
        let mut at = t + Duration::from_millis(300);
        while at < end {
            writes.extend(run_until(ch, t, at));
            ch.key_step(dir, 10.0, at);
            at += Duration::from_millis(40);
        }
        writes.extend(run_until(ch, t, end));
        if release {
            ch.key_release();
        }
        writes.extend(run(ch, t, SCREEN_WRITE));
        writes
    }

    /// Screen value after `n` taps up from the lowest level.
    fn key_level(n: i32) -> u32 {
        let t = Instant::now();
        let mut ch = screen(nits(2));
        for _ in 0..n {
            tap(&mut ch, 1, t);
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
    fn a_tap_glides_one_step_in_about_0_13_seconds() {
        let t = Instant::now();
        let mut ch = screen(nits(300));
        let w = tap(&mut ch, -1, t);
        assert_eq!(w.last().unwrap().1, ch.target);
        let last_t = w.last().unwrap().0;
        assert!(last_t > 0.1 && last_t < 0.2, "{last_t} {w:?}");
        assert!(w.len() >= 6, "{w:?}");
        assert!(step_pcts(nits(300), &w).iter().all(|&p| p < 4.2), "{w:?}");
        assert!(w.windows(2).all(|p| p[0].1 > p[1].1));
        // A manual change: the light is left alone until it changes a lot.
        assert_eq!(ch.update(12.0, 30.0, t), Update::Nothing);
        assert_eq!(ch.update(500.0, 30.0, t), Update::Moving(nits(500)));
    }

    #[test]
    fn holding_a_key_moves_on_until_it_is_let_go() {
        let t = Instant::now();
        let mut ch = screen(nits(20));
        let w = hold(&mut ch, 1, t, 0.8, true);
        assert!(w.windows(2).all(|p| p[0].1 < p[1].1));
        // Full speed from 0.05 s on, about 3.7 % a write in the motion
        // space, with no dip when the repeats start at 0.3 s ...
        let m = motion_pcts(nits(20), &w);
        let steady: Vec<f32> = w.iter().zip(&m).filter(|(x, _)| x.0 > 0.05 && x.0 < 0.78).map(|(_, &p)| p).collect();
        assert!(steady.iter().all(|&p| p > 3.0 && p < 4.0), "{steady:?}");
        // ... and it stops in the frame after the key is let go.
        let end = w.last().unwrap().0;
        assert!(end < 0.83, "{end}");
        // 0.8 s at 2.08 ln/s from 20 nits.
        assert!(ch.ramp.is_none() && ch.known > nits(140) && ch.known < nits(190), "{}", ch.known);
    }

    #[test]
    fn letting_go_early_still_finishes_the_step() {
        let t = Instant::now();
        let mut a = screen(nits(300));
        let tapped = tap(&mut a, -1, t).last().unwrap().1;
        let mut b = screen(nits(300));
        let held = hold(&mut b, -1, t, 0.05, true).last().unwrap().1;
        assert_eq!(held, tapped);
    }

    #[test]
    fn a_lost_release_stops_after_the_timeout() {
        // The last repeat comes at 0.46 s; nothing after it.
        let t = Instant::now();
        let mut ch = screen(nits(20));
        let w = hold(&mut ch, 1, t, 0.5, false);
        let end = w.last().unwrap().0;
        assert!(end > 0.84 && end < 0.92, "{end}");
        assert!(ch.ramp.is_none());
    }

    #[test]
    fn a_press_at_the_bottom_stays_there() {
        let t = Instant::now();
        let mut ch = screen(nits(2));
        assert!(tap(&mut ch, -1, t).is_empty());
        assert_eq!(ch.known, nits(2));
        assert!(hold(&mut ch, -1, t, 0.6, true).is_empty());
        let mut ch = screen(1);
        assert_eq!(ch.key_step(-3, 10.0, t), nits(2));
    }

    #[test]
    fn without_the_kernel_patches_a_change_takes_as_long() {
        // Writes then block for 46 ms, so there are a third as many, each
        // three times the size.
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
        assert!(step_pcts(nits(140), &slow).iter().all(|&p| p < 2.6));
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
        assert!(last_t > 1.5 && last_t < 3.0, "{last_t}");
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
