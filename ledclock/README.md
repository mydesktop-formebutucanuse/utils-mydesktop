# ledclock

A clock on the desktop: the time in red on a dark display panel, in a metal
frame lit from the top left.

```
AM 07:15
```

It is a window of its own, three times as wide as it is high, and not a
dock app. It has no title bar and no border, is on every workspace, takes no
focus, has no application icon and is not in the window list. It stays
under the other windows, like a thing on the desktop; with `-above` it stays
over them, at the level of the Dock.

Drag it with the left button to move it. Drag one of its four corners to
make it larger or smaller: it keeps its proportions, and the opposite corner
stays where it is. The pointer shows which of the two a press will do. The
next time ledclock starts, it is where the pointer left it, at that size.

```sh
ledclock &                      # AM 07:15, where it was the last time
ledclock -geometry -40+120 &    # 40 pixels from the right edge, 120 from the top
ledclock -24 -above &           # 19:15, over the other windows
```

| Option | |
|---|---|
| `-geometry +X+Y` | Where the window is, in pixels from the left and top edges of the screen; `-X` and `-Y` count from the right and bottom edges. Without it: where the pointer left it; the first time, in the middle of the screen's width, half the clock's height from the top |
| `-width PIXELS` | The width of the window, 60 to 8192; the height is 90/274 of it. Without it: the width the pointer gave it; the first time, 274 pixels times Window Maker's GUI scale factor (548x180 at a factor of 2) |
| `-24` | 24 hours, without AM and PM |
| `-above` | Over the other windows instead of under them |
| `-color RRGGBB` | The colour of the time (`ff0000`) |
| `-font FONT` | The font of the digits: a family or a Fontconfig pattern, without a size (`"Liberation Sans:bold"`) |

Window Maker does not move or resize a window of this kind, with Alt and
the mouse or otherwise: ledclock does both itself. It keeps the whole clock
on the screen when it is moved.

The place and the width the pointer gave it are kept in
`~/.local/state/ledclock/placement` (under `$XDG_STATE_HOME` when that is
set), as `X Y WIDTH`. `-geometry` and `-width` come before that file, each
for what it names, every time ledclock starts with them: to have the clock
stay where it is dragged, start it without `-geometry`. Remove the file to
have the first place and size again.

The scale factor is the one Window Maker and [scd](../scd) use: the
environment variable `GS_SCALE_FACTOR`, or else `GSScaleFactor` in
`~/GNUstep/Defaults/WMGLOBAL` (in `$WMAKER_USER_ROOT/Defaults/WMGLOBAL` when
that variable is set). It is read when ledclock starts. The frame and the
panel are computed for the size of the window, not enlarged from a picture,
and the digits are drawn by Xft at that size: the clock is as sharp at 2 as
at 1, and at any size it is dragged to.

The digits are in DejaVu Sans Condensed and AM and PM in DejaVu Sans Mono
when those fonts are installed (`media-fonts/dejavu` on Gentoo): narrow
digits with a foot under the 1, as the design this clock was drawn from has
them. Without them the digits are in the system's sans-serif font and AM and
PM in its monospace one.

ledclock wakes once a minute, at the minute. Its timer follows the wall
clock, so the time is right at once after the machine was suspended or the
clock was set.

No crate dependencies: only libX11 and libXft are linked.

## Build and install

On Gentoo the package installs it (see the [top README](../README.md)).
By hand, with Rust 1.85 or later, libX11 and libXft, from the top of the
repository:

```sh
cargo build --release
install -m 755 target/release/ledclock ~/.local/bin/
```

Start it with the X session, for example in
`~/GNUstep/Library/WindowMaker/autostart`:

```sh
ledclock &
```
