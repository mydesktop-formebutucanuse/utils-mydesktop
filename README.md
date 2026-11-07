# utils-mydesktop

Small desktop utilities, written for a Window Maker (X11) desktop on
Gentoo (Asahi Linux, Apple Silicon MacBook).

| Tool | What it does |
|---|---|
| [mediakeyd](mediakeyd) | Runs commands for volume, brightness and media keys without grabbing them, so focused windows do not flicker |
| [backlight](backlight) | The brightness keys' command: screen steps through autobrightd, keyboard through logind |
| [autobrightd](autobrightd) | Sets the screen and keyboard backlight from the ambient light sensor, and steps aside when you change them |
| [scd](scd) | Gives Window Maker's GUI scale factor to GTK, Chromium, Electron and Qt applications and to the cursors, without scaling any of them twice |
| [logincookies](logincookies) | Keeps LibreWolf's login cookies, and only those, when it deletes its cookies on exit |
| [kernel](kernel) | Asahi kernel patches: the panel backlight in 1/64 nit and up to 60 updates a second, so brightness changes have no visible steps |

## Install on Gentoo

[gentoo](gentoo) has a live ebuild, `x11-misc/utils-mydesktop-9999`, that
builds the latest commit here and installs all the tools: `mediakeyd`,
`autobrightd`, `scd`, `logincookies` and `backlight` in `/usr/bin`,
autobrightd's udev rule in `/usr/lib/udev/rules.d`, and the READMEs. The
kernel patches are separate (see [kernel](kernel)).

```sh
sudo sh gentoo/install.sh          # adds it to the mydesktop overlay and emerges it
sudo emerge --oneshot x11-misc/utils-mydesktop   # later, for the latest code
```

`gentoo/install.sh [OVERLAY]` copies the ebuild into the overlay (default
`mydesktop`), adds `x11-misc/utils-mydesktop **` to
`/etc/portage/package.accept_keywords/OVERLAY`, emerges the package and
removes the copy of the udev rule a hand install left in
`/etc/udev/rules.d`. Run it again when the ebuild changes.

Then start the daemons with the X session, scd first (see each README), for
example in `~/.xinitrc` before `exec wmaker`:

```sh
scd &
mediakeyd &
autobrightd &
```

## Build by hand

The Rust tools are members of one Cargo workspace. From the top of the
repository:

```sh
cargo build --release
```

Binaries are written to `target/release/`. See each tool's README for
installing and configuring it.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
