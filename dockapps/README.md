# dockapps

Two dock apps for Window Maker's Dock, drawn at the GUI scale factor: a
clock and calendar, **wmclock** 1.0.16, and a battery monitor, **wmacpi**
2.4 (with `wmacpi-cli`, the same readings on the command line).

A dock app draws a picture of 64x64 pixels or less. Window Maker makes its
tiles 64 x `GSScaleFactor` pixels wide but does not enlarge the dock app, so
at a factor of 2 the clock is 54x57 pixels in the middle of a tile of
128x128. The two here are the upstream programs with one change: they draw
themselves N times as large, N being the scale factor rounded down (2 for 2
and for 2.5), each pixel as a block of N x N pixels. At a factor below 2,
or on an X server without the RENDER extension, they are what they were.

The factor is the one Window Maker and [scd](../scd) use: the environment
variable `GS_SCALE_FACTOR`, or else `GSScaleFactor` in
`~/GNUstep/Defaults/WMGLOBAL` (in `$WMAKER_USER_ROOT/Defaults/WMGLOBAL` when
that variable is set). It is read when the dock app starts.

## Use

```sh
wmclock &       # -12 for a 12-hour clock, -exe COMMAND to run one on a click
wmacpi -f &     # a click shows the next battery
```

On an Apple Silicon MacBook start the battery monitor with `-f`. Without it
wmacpi takes the time left from the battery's `current_now`, which is
negative while the battery discharges and which wmacpi reads as an unsigned
number: the time shows as 0:00. With `-f` it works the time out from how
fast the charge falls.

To keep them in the Dock, start each once from a terminal, drag its tile
onto the Dock, and in the tile's Settings tick "Start when Window Maker is
started"; for the battery monitor make sure the command there is
`wmacpi -f`.

`man wmclock`, `man wmacpi` and `man wmacpi-cli` have the other options.
wmclock's day and month names in other languages are in
`/usr/share/wmclock`; `man wmclock` shows how to choose one.

## Build

On Gentoo the package builds and installs them (see the
[top README](../README.md)). It takes the place of `x11-plugins/wmclock`
and `x11-plugins/wmacpi`, which install the same files.

By hand, with a C compiler, make, pkg-config, libX11, libXext, libXpm,
libXrender and libdockapp:

```sh
make -C dockapps                      # build/wmclock, build/wmacpi, build/wmacpi-cli
make -C dockapps PREFIX=~/.local install
```

`make -C dockapps WMCLOCK_LANG=french` builds wmclock with the day and month
names of `wmclock/lang.french`. Give `PREFIX` to both commands when they
are run apart: wmclock's man page names the directory of the languages.

## How it is done

[scale.h](scale.h) is all of it. The dock app goes on drawing its own
pixmap at its own size. Its window, the window's size hints and its shape
are N times as large, and the one copy from the pixmap to the window is done
by the RENDER extension, which enlarges it in the X server without smoothing
it. Neither program looks at where a click lands, so the mouse needs
nothing.

The sources are in [wmclock](wmclock) and [wmacpi](wmacpi); what was
changed in them, and whose they are, is in [NOTICE](NOTICE).
