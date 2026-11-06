# kernel

Patches for the Asahi Linux kernel.

| Patch | What it does |
|---|---|
| [0001-drm-apple-backlight-1-16-nit.patch](0001-drm-apple-backlight-1-16-nit.patch) | The panel backlight counts in 1/16 nit instead of whole nits |

## 1/16-nit backlight

`apple-panel-bl` counts in whole nits, and a 1-nit step is easy to see at
low brightness: 20 % at 5 nits, 2 % at 50, 0.8 % at 125. Slow brightness
changes, such as [autobrightd](../autobrightd)'s, then move in visible
steps.

The driver turns nits into a panel DAC value by linear interpolation in two
lookup tables, so finer input costs nothing. With the patch the device
counts in 1/16 nit (`max_brightness` 500 becomes 8000 on a 14-inch MacBook
Pro). Whole nits give exactly the same DAC values as before, and the 15
values between two nits fall on the same lines. This was checked for every
value from 0 to 8144 against the driver's own arithmetic; the patched file
also builds without warnings (`W=1`).

Tools that scale by `max_brightness` (logind, brightnessctl, desktop
sliders, [backlight](../backlight), autobrightd) work as before. A script
that writes raw nits now gets 1/16 of them.

Written against `asahi-7.2.14-1` (Gentoo `sys-kernel/asahi-kernel-7.2.14_p1`).
On Gentoo the ebuild applies user patches, so:

```sh
sudo install -D -m 644 kernel/0001-drm-apple-backlight-1-16-nit.patch \
	/etc/portage/patches/sys-kernel/asahi-kernel/0001-drm-apple-backlight-1-16-nit.patch
sudo emerge --oneshot sys-kernel/asahi-kernel
```

and reboot. Later kernel versions pick the patch up as long as it still
applies; if a build fails on it, check `drivers/gpu/drm/apple/dcp_backlight.c`.

The patch changes a file licensed GPL-2.0-only OR MIT and is under the same
terms.
