# scd

Gives the GUI scale factor of Window Maker (`GSScaleFactor`) to the
applications of the other toolkits: GTK, Chromium, Electron, Qt and the ones
using Xft directly. The cursors follow the factor too.

Only the WINGs applications (Window Maker, WPrefs, the dockapps) read
`GSScaleFactor`. Scaling the other toolkits one by one easily scales some
applications twice: Chromium and Electron multiply the GTK window scale by
`Xft.dpi / 96`, so `GDK_SCALE=2` together with an `Xft.dpi` of 192 makes them
4 times as big. scd does what GNOME does instead:

- It becomes the XSETTINGS manager of the screen. `Gdk/WindowScalingFactor`
  is the whole part of the factor, `Gdk/UnscaledDPI` gives the rest of it to
  the fonts, so they are not scaled twice, and `Xft/DPI` is the full
  resolution. `Gtk/CursorThemeSize` is the cursor size in pixels (24 × factor,
  like `Xcursor.size`): GTK 3 gives it to Xcursor without scaling it again,
  and Chromium and Electron read it before `Xcursor.size`. Chromium 152 makes
  a missing `Gtk/CursorThemeSize` 24, so without it their cursors stayed at
  24 pixels whatever the factor. GTK, Chromium and Electron applications
  follow a change of these settings while they run.
- It sets the `Xft.dpi` (96 × factor) and `Xcursor.size` (24 × factor)
  resources, for Qt, Xft applications and the cursor themes. These are read
  when an application starts. The previous lines are saved on the root window
  and put back when scd exits, even after scd was killed and started again.

At factor 2: GTK windows scale 2 with 96 dpi fonts, Electron and Chromium
have a `devicePixelRatio` of 2 and the cursors are 48 pixels. At factor 1.5:
GTK windows scale 1 with 144 dpi fonts (GTK only scales windows by whole
numbers), `devicePixelRatio` 1.5.

No crate dependencies: only libX11 is linked.

## Build and install

On Gentoo the package installs it (see the [top README](../README.md)).
By hand, with Rust 1.85 or later and libX11, from the top of the repository:

```sh
cargo build --release
install -m 755 target/release/scd ~/.local/bin/
```

## Usage

```
scd [-v] [-c SIZE] [FACTOR]
```

Without `FACTOR`, scd uses the factor Window Maker uses: `GS_SCALE_FACTOR`,
else `GSScaleFactor` in `~/GNUstep/Defaults/WMGLOBAL`, else 1. `-c` sets the
cursor size at factor 1 (24 by default), `-v` prints the settings it gives.

While scd runs, `scd FACTOR` changes its factor: GTK, Chromium and Electron
applications are rescaled at once, applications started afterwards get the
new resources. Window Maker itself only reads `GSScaleFactor` when it starts.

Start it with the X session, before the other applications, for example in
`~/.xinitrc` before `exec wmaker`, or from Window Maker's
`~/GNUstep/Library/WindowMaker/autostart`:

```sh
if command -v scd >/dev/null && ! pgrep -x -u "$(id -u)" scd >/dev/null; then
	scd &
fi
```

Remove the scale factors set by hand for one toolkit (`GDK_SCALE`,
`GDK_DPI_SCALE`, `QT_SCALE_FACTOR`, `Xft.dpi` in `~/.Xresources`, Electron's
`--force-device-scale-factor`): they would be applied on top of scd's. With
`GDK_SCALE` set, GTK reads `Xft/DPI` instead of `Gdk/UnscaledDPI` and
Electron is scaled twice again. Qt 5 also needs
`QT_ENABLE_HIGHDPI_SCALING=1` in the environment of the session to scale from
`Xft.dpi`; Qt 6 does it by default.

Only one XSETTINGS manager can run on a screen: scd exits with an error when
another one (xsettingsd, a desktop environment's settings daemon) runs, and
exits when one takes over. Java applications do not read any of these
settings.

## License

GPL-3.0-or-later. See [LICENSE](../LICENSE).
