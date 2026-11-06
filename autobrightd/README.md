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
| A change | 25 % → 100 % in 73 writes over about 5.7 s; steps up to 17 %, biggest in the dark when rising | at most 0.3 % a write, easing in and out; with the [kernel patch](../kernel), 140 → 315 nits took 296 writes over 14 s, 0.27 % on average (measured, before the dark offset) |
| Runs as | root | your user (udev rule below) |

Writes matter: every screen write makes the Apple DCP driver sleep 42 ms
and run a display commit, and the kernel sends a uevent for it. Measured
on an M1 Pro, one write takes 43 to 47 ms, so a 33 ms tick plus a write is
about 77 ms, not 33. The driver holds a lock while it sleeps, so writes
cannot overlap: about 21 a second is the limit.

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
  roughly 1 % on a large bright field, and the DCP driver takes only about
  21 writes a second, so the only way to make a change invisible is to make
  each step small. A change moves at no more than about 0.3 % per write,
  speeding up over the first 1.5 s and slowing down over the last. It is
  measured in ratios of (brightness + 3 % of the maximum): ratios because
  that is what the eye sees, the 3 % because in the dark the eye needs
  bigger steps and the panel shows nothing below 2 nits, so pure ratios
  would crawl there for minutes. Its length follows its size: about 1.5 s
  for a small change, 15 s for 140 to 315 nits, 25 s for 25 % to 100 %;
  from below 2 nits it starts at 2 nits, which looks the same, so it
  rises from the first moment. A new target during a change keeps the
  speed it has; one in the other direction slows down first, and nothing
  overshoots. Each write is the nearest whole value, which needs the
  [kernel patch](../kernel) (1/16 nit) to stay smooth; with whole nits the
  slow parts move in 1-nit steps. The keyboard moves much faster (off to
  75 % in about 4 s): nobody watches it fade.
- **Brightness keys.** The [backlight](../backlight) command sends SIGUSR1
  (brighter) or SIGUSR2 (dimmer). Each press moves the screen 1/16 of the
  way between 2 nits and the maximum in the same ratio space as
  transitions, so every press looks the same size, and the screen glides
  there in about 0.65 s, in steps of at most 2 % per write that ease in and
  out. Held down (key repeat), it moves steadily and sweeps the whole range
  in about 8 s; presses never run more than two steps ahead, so it stops
  within a second of letting go. Like any manual change, the light is then
  left alone until it changes a lot.
- **Manual changes.** Anything that writes the brightness file (a key bound
  to `brightnessctl`, the [backlight](../backlight) script, logind) is
  noticed by comparing the file with the last value written. That channel is
  then left alone until the light shifts by 75 % and at least 5 lux; then it
  ramps from your value to the automatic one. The screen is watched through
  the kernel's change notification; the keyboard (LED class, which has none)
  is checked with every sample. During a transition a key press usually
  waits for the backlight lock and lands right after one of our writes, so
  each write is read back (a different value means a manual change) and the
  next frame waits 1 ms, by which time a waiting write is in place.
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

```sh
cargo build --release
install -m 755 target/release/autobrightd ~/.local/bin/
sudo install -D -m 644 autobrightd/90-autobrightd.rules /etc/udev/rules.d/90-autobrightd.rules
sudo udevadm trigger --action=add /sys/class/backlight/apple-panel-bl /sys/class/leds/kbd_backlight
```

Try it in the foreground first. `-n` reads the sensor and prints what it
would do without writing anything:

```sh
autobrightd -v -n
autobrightd -v
```

Start it with the session, e.g. from `~/GNUstep/Library/WindowMaker/autostart`:

```sh
if [ -x "$HOME/.local/bin/autobrightd" ] && ! pgrep -x -u "$(id -u)" autobrightd >/dev/null; then
	"$HOME/.local/bin/autobrightd" &
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
