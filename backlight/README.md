# backlight

The brightness keys' command. [mediakeyd](../mediakeyd) runs it. On
Gentoo the package installs it (see the [top README](../README.md)); by hand,
`install -m 755 backlight/backlight ~/.local/bin/`.

```sh
backlight screen +5     # one step brighter
backlight screen -5     # one step dimmer
backlight screen release   # the key was let go: stop
backlight kbd +10       # keyboard backlight, +10 % of its maximum
backlight kbd -10
```

The screen is [autobrightd](../autobrightd)'s, which has to be running:
`backlight screen` only sends it SIGUSR1 (brighter), SIGUSR2 (dimmer) or,
with `release`, SIGRTMIN (the key was let go). autobrightd moves the screen
1/16 of the way between 2 nits and the maximum per tap, in steps that look
the same size to the eye, gliding there in about 0.12 s; held down, it
moves on until the key is let go (the whole range in about 1.6 s). The
number after the sign only gives the direction. Bind `backlight screen
release` to the keys' release in [mediakeyd](../mediakeyd) (see its
config.example).

The keyboard backlight is set through elogind's (or systemd-logind's)
`Session.SetBrightness` on the system D-Bus, which needs no root; the
caller must own the active session.

Uses `sh`, `cat`, `pkill` and `dbus-send`.
