# notchbar

Shows the battery level and the time beside the camera notch of the
MacBook, the way the macOS menu bar does:

    80%  Tue Nov 10  2:32 PM

The Asahi kernel hides the notch by default: the panel is cut to 16:10 below
it (3024×1890 on the 14" MacBook Pro). With `appledrm.show_notch=1` X gets
the whole panel (3024×1964) and the 74 pixel strip on both sides of the
notch becomes usable. notchbar fills that strip with a dock window that is
black at the top, around the notch, and fades into the desktop background
further down, and reserves it with `_NET_WM_STRUT`, so Window Maker places
and maximises windows below it. The strip's height is worked out from the
screen size (screen height less 10/16 of its width), so the 16" and the
notched Airs need no option either.

Fullscreen windows ignore `_NET_WM_STRUT` and fill the whole screen, notch
included. notchbar also sets `_WINDOWMAKER_FULLSCREEN_STRUT` (laid out like
`_NET_WM_STRUT`), which the
[mydesktop Window Maker](https://github.com/mydesktop-formebutucanuse/wmaker-mydesktop)
reads to place fullscreen windows below the strip too (3024×1890 at +0+74),
moving those already fullscreen when notchbar starts or quits. Other window
managers ignore it.

The fade starts lightening right at the top edge of the screen and runs on
below the strip, for 3/4 of its height by default (56 pixels). It meets the
desktop background flat, so its end does not show as an edge. The part
below the strip is an override-redirect window kept under every other
window: it is seen only where the desktop is, and it takes no clicks, so
the root menu still opens there. The background is the root window's
pixmap (`_XROOTPMAP_ID`, as set by wmsetbg, feh and the like), darkened row
by row; it is read again when the background changes. Without one the
strip and the fade are plain black.

The battery level is read from `/sys/class/power_supply` every 5 seconds
and the clock is redrawn at every minute.

## Showing the notch

[kernel/appledrm.conf](../kernel/appledrm.conf) sets `appledrm.show_notch=1`
(see [kernel](../kernel)): `sudo sh kernel/install.sh`, rebuild the kernel so
that the option reaches the initramfs, and reboot.
`cat /sys/module/appledrm/parameters/show_notch` then prints `Y` and the
screen is 3024×1964. Without notchbar the strip is plain screen: windows may
be placed under the notch.

## Usage

Start it with the session, e.g. from `~/GNUstep/Library/WindowMaker/autostart`,
so that a Window Maker restart does not start a second one:

```sh
if command -v notchbar >/dev/null && ! pgrep -x -u "$(id -u)" notchbar >/dev/null; then
	notchbar &
fi
```

| Option | |
|---|---|
| `-f FORMAT` | strftime(3) format of the clock, default `%a %b %-d  %-I:%M %p`. The names follow `LC_TIME`; e.g. `-f '%-m월 %-d일 (%a) %p %-I:%M'` with a Korean `LC_TIME`. |
| `-F FONT` | Fontconfig pattern, default `Noto Sans CJK KR:medium`. The pixel size follows the strip (26 at 74 pixels) unless the pattern gives one. |
| `-P` | No battery level, the clock alone. |
| `-g FADE` | How far the fade runs on below the strip, in pixels, default 3/4 of the strip's height. With `-g 0` it ends at the strip's bottom edge, and the text sits on a lighter background. |
| `-H HEIGHT` | Height of the strip, for screens whose notch is not found. On a screen without a notch strip notchbar exits unless this is given. |

Nothing is drawn on the left of the notch.
