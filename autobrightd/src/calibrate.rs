// SPDX-License-Identifier: GPL-3.0-or-later
//! `autobrightd --calibrate`: measures how the brightness the panel shows
//! follows the value written, and saves it for the daemon (logic::PanelMap).
//!
//! The DCP reports the brightness it shows in actual_brightness, in whole
//! nits and about 25 ms late. The screen is swept slowly from its lowest
//! level to the maximum and back. Going up, a reported value is reached
//! when the brightness gets there; going down, it is left when the
//! brightness drops below it. Halfway between the two written values (in
//! ratio) is where the panel shows exactly that value: the delay and the
//! rounding down cancel out.

use crate::logic::{self, PanelMap};
use crate::sys::Attr;
use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

/// Sweep speed in ln(units + dark offset) a second: about 34 s each way.
const SPEED: f32 = 0.1;
const FRAME: Duration = Duration::from_millis(16);
/// Pause at either end, so the reports catch up.
const SETTLE: Duration = Duration::from_secs(1);
/// Fewer usable points than this means the panel did not report.
const MIN_POINTS: usize = 20;

/// Where the map is kept: $XDG_STATE_HOME/autobrightd/panel.
pub fn path() -> Option<PathBuf> {
    let base = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".local/state"),
    };
    Some(base.join("autobrightd/panel"))
}

/// The saved map, if there is one. It has to be for this max_brightness
/// (the kernel patches change the units).
pub fn load(max: u32) -> Result<Option<PanelMap>, String> {
    let Some(path) = path() else { return Ok(None) };
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    let (mut points, mut made_for) = (Vec::new(), None);
    for line in text.lines() {
        if let Some(m) = line.strip_prefix("# max_brightness ") {
            made_for = m.trim().parse::<u32>().ok();
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut words = line.split_whitespace().map(str::parse::<f32>);
        match (words.next(), words.next()) {
            (Some(Ok(w)), Some(Ok(s))) => points.push((w, s)),
            _ => return Err(format!("{}: bad line `{line}`", path.display())),
        }
    }
    if made_for != Some(max) {
        return Err(format!(
            "{} was made for another max_brightness; run autobrightd --calibrate again",
            path.display()
        ));
    }
    match PanelMap::new(points) {
        Some(map) if map.len() >= MIN_POINTS => Ok(Some(map)),
        _ => Err(format!("{}: too few points", path.display())),
    }
}

/// Whether another autobrightd runs: it would fight over the brightness.
fn daemon_running() -> bool {
    let me = std::process::id().to_string();
    let Ok(dir) = fs::read_dir("/proc") else { return false };
    dir.flatten().any(|e| {
        let pid = e.file_name().to_string_lossy().into_owned();
        pid.bytes().all(|b| b.is_ascii_digit())
            && pid != me
            && fs::read_to_string(e.path().join("comm")).is_ok_and(|c| c.trim_end() == "autobrightd")
    })
}

/// Sweeps the screen up and down, saves the map, and puts the brightness
/// back as it was.
pub fn run(dir: &Path) -> Result<(), String> {
    if daemon_running() {
        return Err("autobrightd is running: stop it first (pkill -x autobrightd)".into());
    }
    let max: u32 = crate::sys::read_once(&dir.join("max_brightness"))
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or("cannot read max_brightness")?;
    let open = |name: &str, write| Attr::open(dir.join(name), write).map_err(|e| format!("{name}: {e}"));
    let (set, shown) = (open("brightness", true)?, open("actual_brightness", false)?);
    let before = set.read_u32().map_err(|e| format!("brightness: {e}"))?;

    let off = max as f32 * logic::DARK_PCT / 100.0;
    let lowest = (max as f32 * logic::SCREEN_DEAD_PCT / 100.0) as u32;
    let (a, b) = ((lowest as f32 + off).ln(), (max as f32 + off).ln());
    let secs = (b - a) / SPEED;
    println!("Sweeping the screen from its lowest level to the maximum and back, {secs:.0} s each way.");
    let result = (|| {
        set.write_u32(lowest).map_err(|e| format!("brightness: {e}"))?;
        thread::sleep(SETTLE);
        let up = sweep(&set, &shown, (a, b), secs, off)?;
        thread::sleep(SETTLE);
        let down = sweep(&set, &shown, (b, a), secs, off)?;
        Ok::<_, String>((up, down))
    })();
    let _ = set.write_u32(before);
    let (up, down) = result?;

    let (mut points, mut delays) = (Vec::new(), Vec::new());
    for (level, &w_up) in &up {
        if let Some(&w_down) = down.get(level) {
            let (x_up, x_down) = ((w_up + off).ln(), (w_down + off).ln());
            points.push((((x_up + x_down) / 2.0).exp() - off, *level as f32));
            delays.push((x_up - x_down) / 2.0 / SPEED * 1000.0);
        }
    }
    let map = PanelMap::new(points).filter(|m| m.len() >= MIN_POINTS).ok_or_else(|| {
        format!("only {} usable points: does the panel report its brightness?", up.len().min(down.len()))
    })?;
    delays.sort_by(f32::total_cmp);

    let path = path().ok_or("no HOME")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let mut text = format!(
        "# autobrightd panel map, made by autobrightd --calibrate\n\
         # max_brightness {max}\n\
         # written value, value the panel then reports (actual_brightness)\n"
    );
    for (w, s) in map.points() {
        text.push_str(&format!("{w:.1} {s:.0}\n"));
    }
    fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "{} points, reported about {:.0} ms late; saved to {}.\nRestart autobrightd to use it.",
        map.len(),
        delays[delays.len() / 2],
        path.display()
    );
    Ok(())
}

/// One way of the sweep, from `x.0` to `x.1` (ln units) in `secs`. Returns
/// for each reported value the value written when it was reached (going
/// up) or left (going down).
fn sweep(set: &Attr, shown: &Attr, x: (f32, f32), secs: f32, off: f32) -> Result<HashMap<u32, f32>, String> {
    let up = x.1 > x.0;
    let read = || shown.read_u32().map_err(|e| format!("actual_brightness: {e}"));
    let mut levels = HashMap::new();
    let mut last = read()?;
    let start = Instant::now();
    loop {
        let f = (start.elapsed().as_secs_f32() / secs).min(1.0);
        let value = ((x.0 + (x.1 - x.0) * f).exp() - off).round().max(0.0);
        set.write_u32(value as u32).map_err(|e| format!("brightness: {e}"))?;
        let now = read()?;
        if up && now > last {
            levels.entry(now).or_insert(value);
        } else if !up && now < last {
            levels.entry(last).or_insert(value);
        }
        last = now;
        if f >= 1.0 {
            return Ok(levels);
        }
        thread::sleep(FRAME);
    }
}
