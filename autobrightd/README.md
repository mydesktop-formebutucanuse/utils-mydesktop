# autobrightd

Ambient-light auto-brightness for Apple Silicon MacBooks on Asahi Linux.
It drives the screen and keyboard backlights from the AOP ambient light
sensor, and steps aside when you change either one yourself.

It follows the design of
[asahi-brightnessd](https://github.com/craig-miller/asahi-brightnessd) (same
curves, AC boost, override rule and transition step) but costs less while idle,
writes less often, and runs transitions faster:

| | asahi-brightnessd | autobrightd |
|---|---|---|
| Wakeups while idle | 30 per second | 5 per second (sensor samples only) |
| sysfs files opened while idle | about 120 per second | none (kept open, read with `pread`) |
| Noticing a manual screen change | next 33 ms tick | at once (`POLLPRI` on `actual_brightness`) |
| Noticing a manual keyboard change | next 33 ms tick | next sample, at most 200 ms |
| Noticing the charger | `macsmc-ac/online` once a second, which misses it on an M1 Pro | at once (uevent), from the battery status |
| Writes for lux noise (e.g. 235 ↔ 240 lux) | a few each time | none (2 % dead band) |
| A full 25 % → 100 % rise | 73 writes, about 5.7 s (13 a second), fastest in the dark | the same 73 steps in reverse order, about 3.5 s (21 a second), slowest in the dark |
| Runs as | root | your user (udev rule below) |

Writes matter: every screen write makes the Apple DCP driver sleep 42 ms
and run a display commit, and the kernel sends a uevent for it. Measured
on an M1 Pro, one write takes 43 to 47 ms, so a 33 ms tick plus a write is
about 77 ms, not 33. The driver holds a lock while it sleeps, so writes
cannot overlap: 22 a second is the limit, and autobrightd runs frames back
to back at that rate.

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
- **Transitions.** Falling is asahi-brightnessd's curve: each frame moves
  1/16 of the remaining gap, at least one unit, so it starts fast at the
  bright end and creeps into the dark. Rising is asahi-brightnessd's own
  rise played backwards in time: the same steps, smallest first, so it
  creeps out of the dark one unit at a time and speeds up toward the bright
  end, where steps are harder to see. Either way the dark part moves
  slowly. Frames are 45 ms apart (about 21 a second), as fast as the DCP
  driver takes writes. A new target during a change redirects it.
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
dead band, frame time, transition step and the override thresholds are constants at the top
of [src/logic.rs](src/logic.rs); device paths and timings are at the top of
[src/main.rs](src/main.rs). Edit and rebuild.

## License

GPL-3.0-or-later, like the rest of the repository. See [LICENSE](../LICENSE).

The curve points, the transition step and the override rule come from
asahi-brightnessd, which adapted them from juicecultus/asahi-auto-brightness.
Both are MIT licensed, and the MIT license asks that their copyright notice
stay with copies of that work; it is in [NOTICE](NOTICE).
