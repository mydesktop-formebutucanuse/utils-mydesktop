# wmutils

Small desktop utilities, written for a Window Maker (X11) desktop on
Gentoo (Asahi Linux, Apple Silicon MacBook).

| Tool | What it does |
|---|---|
| [mediakeyd](mediakeyd) | Runs commands for volume, brightness and media keys without grabbing them, so focused windows do not flicker |
| [backlight](backlight) | The brightness keys' command: screen steps through autobrightd, keyboard through logind |
| [autobrightd](autobrightd) | Sets the screen and keyboard backlight from the ambient light sensor, and steps aside when you change them |
| [scd](scd) | Gives Window Maker's GUI scale factor to GTK, Chromium, Electron and Qt applications and to the cursors, without scaling any of them twice |
| [kernel](kernel) | Asahi kernel patches: the panel backlight in 1/64 nit and up to 60 updates a second, so brightness changes have no visible steps; the PMP on for lower idle power; ProMotion VRR; the full panel height around the notch; a smaller kernel config, without the drivers this MacBook cannot use, that builds 2156 modules instead of 4946 |
| [audio](audio) | Puts the sound system ahead of everything else on the CPU: PipeWire and asahi-audio's speaker DSP run in real time, above every other program and interrupt thread |
| [network](network) | Keeps the machine from being found or reached on the networks it joins: a firewall that lets in nothing unasked and does not answer ping, and no hostname or real MAC address given to the network |
| [nsrun](nsrun) | Runs a program in Linux namespaces of its own, with a home of its own, so that it cannot read or change the rest of the system; with XLibre's Xnamespace extension its windows can be cut off from the others too |
| [selinux](selinux) | Brings SELinux up in permissive mode, one step at a time: the Portage profile, the tools and the policy, the kernel command line and the initramfs, the labels |

## Install on Gentoo

[gentoo](gentoo) has a live ebuild, `x11-misc/wmutils-9999`, that
builds the latest commit here and installs all the tools: `mediakeyd`,
`autobrightd`, `scd`, `backlight`, `nsrun` and `nsrun-xserver`
in `/usr/bin`, autobrightd's udev rule
in `/usr/lib/udev/rules.d`, and the READMEs. The kernel patches and config,
the audio priorities, the firewall and network settings and SELinux are
separate (see [kernel](kernel), [audio](audio), [network](network) and
[selinux](selinux)).

```sh
sudo sh gentoo/install.sh          # adds it to the mydesktop overlay and emerges it
sudo emerge --oneshot x11-misc/wmutils   # later, for the latest code
```

`gentoo/install.sh [OVERLAY]` copies the ebuild into the overlay (default
`mydesktop`), adds `x11-misc/wmutils **` to
`/etc/portage/package.accept_keywords/OVERLAY`, emerges the package and
removes the copy of the udev rule a hand install left in
`/etc/udev/rules.d`. Run it again when the ebuild changes.

Until November 2026 this was utils-mydesktop, and the package
`x11-misc/utils-mydesktop`. `gentoo/install.sh` replaces that package with
this one, and the other install scripts ([audio](audio), [kernel](kernel),
[network](network), [selinux](selinux)) rename or remove what they
installed under the old name the next time they are run.

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
