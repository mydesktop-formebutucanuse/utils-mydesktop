# autobrightd

Ambient-light auto-brightness for Apple Silicon MacBooks on Asahi Linux.
It drives the screen and keyboard backlights from the AOP ambient light
sensor, and steps aside when you change either one yourself.

It follows the design of
[asahi-brightnessd](https://github.com/craig-miller/asahi-brightnessd) (same
curves, AC boost and override rule) but costs less while idle,
writes less often, and changes brightness in steps too small to see:

| | asahi-brightnessd | autobrightd |
|---|---|---|
| Wakeups while idle | 30 per second | 5 per second (sensor samples only) |
| sysfs files opened while idle | about 120 per second | none (kept open, read with `pread`) |
| Noticing a manual screen change | next 33 ms tick | at once (`POLLPRI` on `actual_brightness`) |
| Noticing a manual keyboard change | next 33 ms tick | next sample, at most 200 ms |
| Noticing the charger | `macsmc-ac/online` once a second, which misses it on an M1 Pro | at once (uevent), from the battery status |
| Writes for lux noise (e.g. 235 ↔ 240 lux) | a few each time | none (2 % dead band) |
| A change | 25 % → 100 % in 73 writes over about 5.7 s; steps up to 17 %, biggest in the dark when rising | the lowest level to the maximum in about 8 s, easing in and out, at most 0.8 % a write with the [kernel patches](../kernel); with a panel map, even in what the panel shows |
| Runs as | root | your user (udev rule below) |

Writes matter: without the [kernel patches](../kernel), every screen write
makes the Apple DCP driver sleep 42 ms and run a display commit, and the
kernel sends a uevent for it. Measured on an M1 Pro, one write takes 43 to
47 ms, so a 33 ms tick plus a write is about 77 ms, not 33. The driver
holds a lock while it sleeps, so writes cannot overlap: about 21 a second
is the limit. With the patches a write returns at once and the driver
takes about 60 a second.

## How it works

- **Sampling.** The sensor reports every 200 ms, so lux is read 5 times a
  second. The value is smoothed over `ln(1 + lux)` (eyes respond to ratios)
  with a time constant of about 0.6 s.
- **Curves.** The smoothed lux maps to a screen and a keyboard percentage
  through two piecewise-linear curves. The screen rises with light (25 %
  floor, +30 points on AC); the keyboard does the opposite and is off from
  50 lux up.
- **Dead band.** A new target is acted on only if it is at least 2 % of
  `max_brightness` away from the current one, or if it is 0.
- **Transitions.** Around 20 Hz the eye picks up a brightness step of
  roughly 1 % on a large bright field, so a change is made of many small
  steps. Every change moves at the same speed, measured in ratios of
  (brightness + 3 % of the maximum): ratios because that is what the eye
  sees, the 3 % because in the dark the eye needs bigger steps and the
  panel shows nothing below 2 nits, so pure ratios would crawl there for
  minutes. From the lowest level to the maximum takes about 8 s: top speed
  0.47 a second in that space, reached and given up over 0.75 s, 0.8 % per
  write at 60 writes a second with the [kernel patches](../kernel) (without
  them the DCP driver takes about 21 writes a second, and the same change
  takes as long in steps of 2.2 %). A small change takes about 0.35 s, 140
  to 315 nits 2.4 s, 25 % to 100 % 3.5 s. From below 2 nits it starts at
  2 nits, which looks the same, so it rises from the first moment. A new
  target during a change keeps the speed it has (it never slows down
  because the light moved); one in the other direction slows down first,
  and nothing overshoots. Each write is the nearest whole value, which needs the
  kernel patch for 1/64 nit to stay smooth; with whole nits the slow parts
  move in 1-nit steps. The keyboard moves much faster (off to
  75 % in about 2 s): nobody watches it fade.
- **Panel map.** The panel does not follow the driver's nits evenly. What
  the DCP reports it shows (`actual_brightness`, whole nits, about 26 ms
  late) rises 1.4 times as fast as the written value just above about 130
  and 255 nits and only 0.7 times as fast near the top (measured on an M1
  Pro), so an even change of the written value lurches there.
  `autobrightd --calibrate` measures this once: with autobrightd stopped it
  sweeps the screen from its lowest level to the maximum and back (34 s
  each way) and notes, for every reported nit, the written value where it
  was reached going up and where it was left going down; halfway between
  the two is where the panel shows exactly that nit, the report's delay
  and rounding cancelling out (494 points on the M1 Pro). It is saved in
  `$XDG_STATE_HOME/autobrightd/panel` (`~/.local/state/autobrightd/panel`)
  and read at startup; changes and key steps then move evenly in what the
  panel shows. Measured during a change from 175 to 450 nits at full
  speed, the reported brightness changed at 0.88 to 1.15 times its mean
  rate with the map (the rest is the reports' 1-nit rounding) and at 0.54
  to 1.44 times without. A map made for another `max_brightness` (other
  kernel patches) is ignored with a message: calibrate again.
- **Brightness keys.** The [backlight](../backlight) command sends SIGUSR1
  (brighter) or SIGUSR2 (dimmer) for a press or repeat, and SIGRTMIN when
  the key is let go ([mediakeyd](../mediakeyd)'s `release` binding). A tap
  moves the screen 1/16 of the way between 2 nits and the maximum in the
  same ratio space as transitions, so every tap looks the same size,
  gliding there in about 0.12 s (7 writes). Held down, the screen moves on
  at that speed, 2.08 ln units a second (the whole range in about 1.6 s),
  until the key is let go, and then stops at once: measured with XTest key
  events on the M1 Pro, the last write came 32 ms after the release, and
  the speed did not dip while the key was held. Letting go before the first
  step is done still finishes it. If no release comes (a binding without
  `release`), a key that sends no repeat for 0.4 s counts as let go. Like
  any manual change, the light is then left alone until it changes a lot.
- **Manual changes.** Anything that writes the brightness file (a key bound
  to `brightnessctl`, the [backlight](../backlight) script, logind) is
  noticed by comparing the file with the last value written. That channel is
  then left alone until the light shifts by 75 % and at least 5 lux; then it
  ramps from your value to the automatic one. The screen is watched through
  the kernel's change notification; the keyboard (LED class, which has none)
  is checked with every sample. Without the kernel patches, a write by
  someone else during a transition usually waits for the backlight lock and
  lands right after one of ours, so each write is read back (a different
  value means a manual change) and the next frame waits 1 ms, by which time
  a waiting write is in place.
- **Power source.** The AC boost needs to know whether the charger is
  connected. `macsmc-ac/online` cannot tell on an M1 Pro: it drops to 0 for
  about 0.4 s when the cable goes in or out and reads 1 otherwise, so a
  daemon polling it dims for a second and brightens again. The battery
  status is right (`Discharging` without the charger) but costs about
  0.8 ms of SMC traffic per read, so it is read only when the SMC driver
  sends a `power_supply` uevent, which it does on every plug and unplug,
  and once more 1.5 s later.
- **Panel off.** When the internal panel's DPMS is not `On`, the keyboard is
  switched off, ramps stop, and only DPMS is checked, once a second.

## Requirements

- An Asahi kernel with `CONFIG_IIO_AOP_SENSOR_ALS`, so that an IIO device
  named `aop-sensors-als` appears.
- The sensor calibration at `/lib/firmware/apple/aop-als-cal.bin`. Newer
  Asahi installs put it in the vendor firmware on the EFI partition
  (`vendorfw/firmware.tar`); otherwise
  [juicecultus/asahi-auto-brightness](https://github.com/juicecultus/asahi-auto-brightness)
  explains how to extract it from macOS. If you add the file after boot,
  reboot: when the driver fails at boot it leaves a stale listener behind,
  and reloading the module then gives a sensor that always reads 0.
- To run without root: membership in the `video` group and the udev rule.

## Install

On Gentoo the package installs it with the udev rule (see the
[top README](../README.md)). By hand:

```sh
cargo build --release
install -m 755 target/release/autobrightd ~/.local/bin/
sudo install -D -m 644 autobrightd/90-autobrightd.rules /etc/udev/rules.d/90-autobrightd.rules
sudo udevadm trigger --action=add /sys/class/backlight/apple-panel-bl /sys/class/leds/kbd_backlight
```

Then measure the panel once (the screen sweeps up and down for about
70 s), with autobrightd stopped:

```sh
pkill -x autobrightd
autobrightd --calibrate
```

Try it in the foreground first. `-n` reads the sensor and prints what it
would do without writing anything:

```sh
autobrightd -v -n
autobrightd -v
```

Start it with the session, e.g. from `~/GNUstep/Library/WindowMaker/autostart`:

```sh
if command -v autobrightd >/dev/null && ! pgrep -x -u "$(id -u)" autobrightd >/dev/null; then
	autobrightd &
fi
```

## Tuning

There is no configuration file. Curves, floors, the AC boost, smoothing,
dead band, transition speeds (`SCREEN_MOTION`, `KBD_MOTION`, `DARK_PCT`) and
the override thresholds are constants at the top of [src/logic.rs](src/logic.rs);
device paths and timings are at the top of [src/main.rs](src/main.rs). Edit
and rebuild.

## License

GPL-3.0-or-later, like the rest of the repository. See [LICENSE](../LICENSE).

The curve points and the override rule come from asahi-brightnessd,
which adapted them from juicecultus/asahi-auto-brightness.
Both are MIT licensed, and the MIT license asks that their copyright notice
stay with copies of that work; it is in [NOTICE](NOTICE).
