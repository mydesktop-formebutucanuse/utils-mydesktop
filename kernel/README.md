# kernel

Patches for the Asahi Linux kernel's panel backlight, so that brightness
changes have no visible steps.

| File | What it is |
|---|---|
| [0001-drm-apple-backlight-1-64-nit.patch](0001-drm-apple-backlight-1-64-nit.patch) | The panel backlight counts in 1/64 nit instead of whole nits |
| [0002-drm-apple-backlight-without-blocking.patch](0002-drm-apple-backlight-without-blocking.patch) | A brightness write returns at once, and the brightness can change about 60 times a second instead of 21 |
| [install.sh](install.sh) | Makes every Gentoo `asahi-kernel` build apply them |
| [asahi-kernel.bashrc](asahi-kernel.bashrc) | The Portage hook that install.sh puts in place |

Each patch works without the other. Both change only
`drivers/gpu/drm/apple/dcp_backlight.c`, which serves the built-in panel
only: external displays have no backlight device and are not affected.

Written against `asahi-7.2.14-1` (Gentoo `sys-kernel/asahi-kernel-7.2.14_p1`).

## 1/64-nit backlight

`apple-panel-bl` counts in whole nits, and a 1-nit step is easy to see at
low brightness: 20 % at 5 nits, 2 % at 50, 0.8 % at 125. Slow brightness
changes, such as [autobrightd](../autobrightd)'s, then move in visible
steps.

The driver turns nits into a panel DAC value by linear interpolation in two
lookup tables, so finer input costs nothing. With the patch the device
counts in 1/64 nit (`max_brightness` 500 becomes 32000 on a 14-inch MacBook
Pro). Whole nits give exactly the same DAC values as before, and the 63
values between two nits fall on the same lines. This was checked for every
value from 0 to 32576 against the driver's own arithmetic: whole nits match
the old DAC bit for bit, and the DAC rises with every step from 2 nits up.

1/64 rather than 1/16 because of the second patch: at 60 updates a second a
slow change moves well under 1/16 nit per update in the dark, and would go
in uneven 1/16-nit jumps (3 % at 2 nits).

Tools that scale by `max_brightness` (logind, brightnessctl, desktop
sliders, [backlight](../backlight), autobrightd) work as before. A script
that writes raw nits now gets 1/64 of them.

## Brightness writes without blocking

Every write to the backlight made the driver sleep 42 ms (one frame at
23.976 Hz, in the hope that a display update carries the new brightness
meanwhile) and then run a display commit, all inside the write and under
the backlight lock. A write took 43 to 47 ms on an M1 Pro, so the
brightness could change at most about 21 times a second.

With the patch the write returns at once, and the driver's existing work
item applies it. It waits one frame at 120 Hz (8.4 ms) instead of 42 ms:

| | Before | After |
|---|---|---|
| A write | 43 to 47 ms | returns at once |
| Brightness updates, screen changing | about 21 a second | up to 120 a second: the next frame carries the brightness, no extra commit |
| Brightness updates, screen still | about 21 a second | about 60 a second: wait a frame, then commit |
| Writes faster than the display | wait their turn | merged; the latest value is shown |

On suspend the update is still finished before the backlight core goes on,
so it happens before the display suspends, as before. The writer no longer
sees errors from the display commit.

Costs, measured on an M1 Pro before the patch: each write costs about
0.5 ms of CPU, most of it udev handling the uevent the backlight core sends
for every write. At 60 writes a second that is about 3 % of one core while
the brightness is changing, and nothing while it is not.

## Applying them automatically on Gentoo

```sh
sudo sh kernel/install.sh
sudo emerge --oneshot sys-kernel/asahi-kernel   # now, or wait for the next update
```

and reboot. `install.sh` puts the patches in
`/usr/local/share/utils-mydesktop` (removing any there that left the
series) and a Portage hook in `/etc/portage/env/sys-kernel/asahi-kernel`.
From then on every `asahi-kernel` build, kernel updates included, runs the
hook after the kernel's own patches. For each patch in turn it applies the
patch if it fits, skips it if the source already has it, and otherwise
warns and builds without it. A kernel update therefore never fails because
of a patch; a plain `/etc/portage/patches` entry would stop the build
instead, so `install.sh` removes one if it finds it. It will not overwrite
a hook file it did not write. Run it again after the patches change.
`sudo sh kernel/install.sh remove` undoes it.

If a build warns that a patch no longer fits, that patch is missing until it
is updated for the new `dcp_backlight.c`: without the first the backlight
counts whole nits (and [autobrightd](../autobrightd) shows 1-nit steps),
without the second brightness changes are back to about 21 updates a
second.

The patches change a file licensed GPL-2.0-only OR MIT and are under the
same terms.
