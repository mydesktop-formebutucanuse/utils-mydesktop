# mediakeyd

Run commands for media keys on X11 without grabbing them.

Hotkey daemons such as sxhkd or xbindkeys use `XGrabKey`. Every time a
grabbed key is pressed, the X server sends `FocusOut`/`FocusIn`
(`NotifyGrab`/`NotifyUngrab`) to the focused window. GTK applications such as
Firefox or LibreWolf take that as a real focus change: open menus and popups
close, input method composition is cut off and the window turns inactive for
a moment.

mediakeyd only listens to XInput2 raw key events on the root window. Nothing
is grabbed, so no focus events are generated. The focused application still
receives the key as usual.

- Keys marked `repeat` repeat at the X server's repeat delay and rate
  (`xset r rate`) while they are held down. Raw events carry no autorepeat,
  so mediakeyd does it itself.
- A key's command never runs twice at the same time. Presses that arrive
  while it runs are merged into one more run, so slow commands such as
  backlight changes do not pile up.
- Keyboard map changes are picked up.
- No crate dependencies: only libX11 and libXi are linked.

## Build and install

Needs Rust 1.85 or later, libX11, libXi and an X server with XInput 2.1 or
later. From the top of the repository:

On Gentoo the package installs it (see the [top README](../README.md)).
By hand:

```sh
cargo build --release
install -m 755 target/release/mediakeyd ~/.local/bin/
```

Then the configuration (from `/usr/share/doc/utils-mydesktop-9999/mediakeyd/`
with the package):

```sh
install -D -m 644 mediakeyd/config.example ~/.config/mediakeyd/config
```

## Configuration

`$XDG_CONFIG_HOME/mediakeyd/config` (`~/.config/mediakeyd/config`), or the
file given with `-c`. One binding per line:

```
KEYSYM  repeat|once  COMMAND
```

`COMMAND` runs with `/bin/sh -c`. Lines starting with `#` are comments.
See [config.example](config.example), which controls PipeWire volume with
`wpctl`, media players with `playerctl`, and the screen and keyboard
backlights with [backlight](../backlight).

## Usage

```
mediakeyd [-v] [-c CONFIG]
```

`-v` prints every bound key press. The config is read at startup; restart
mediakeyd after changing it.

Start it with the X session, for example from Window Maker's
`~/GNUstep/Library/WindowMaker/autostart`:

```sh
if command -v mediakeyd >/dev/null && ! pgrep -x -u "$(id -u)" mediakeyd >/dev/null; then
	mediakeyd &
fi
```

Because keys are not grabbed, the focused application also gets them. Most
applications ignore media keys; web pages see them as `keydown` events such
as `AudioVolumeUp` or `MediaPlayPause`.

## License

GPL-3.0-or-later. See [LICENSE](../LICENSE).
