# backlight

The brightness keys' command. [mediakeyd](../mediakeyd) runs it.

```sh
install -m 755 backlight/backlight ~/.local/bin/
backlight screen +5     # one step brighter
backlight screen -5     # one step dimmer
backlight kbd +10       # keyboard backlight, +10 % of its maximum
backlight kbd -10
```

The screen is [autobrightd](../autobrightd)'s, which has to be running:
`backlight screen` only sends it SIGUSR1 (brighter) or SIGUSR2 (dimmer).
autobrightd moves the screen 1/16 of the way between 2 nits and the
maximum per press, in steps that look the same size to the eye, gliding
there in about 0.3 s; held down, the key sweeps the whole range in about
1.7 s. The number after the sign only gives the direction.

The keyboard backlight is set through elogind's (or systemd-logind's)
`Session.SetBrightness` on the system D-Bus, which needs no root; the
caller must own the active session.

Uses `sh`, `cat`, `pkill` and `dbus-send`.
