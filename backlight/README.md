# backlight

Change the screen or keyboard backlight by a percentage of its maximum.

It asks elogind (or systemd-logind) through `Session.SetBrightness` on the
system D-Bus, so it needs no root and no udev rule; the caller must own the
active session. Only `sh`, `cat` and `dbus-send` are used.

```sh
install -m 755 backlight/backlight ~/.local/bin/
backlight screen +5     # first device in /sys/class/backlight
backlight screen -5
backlight kbd +10       # /sys/class/leds/kbd_backlight
backlight kbd -10
```

The screen stops at 0.4 % of its maximum (at least 1): 2 nits on a 500-nit
Apple panel, whose driver shows everything from 0 to 2 nits at the same
lowest level.
[mediakeyd](../mediakeyd) uses it for the brightness keys.
