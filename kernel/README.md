# kernel

Patches for the Asahi Linux kernel.

| File | What it is |
|---|---|
| [0001-drm-apple-backlight-1-16-nit.patch](0001-drm-apple-backlight-1-16-nit.patch) | The panel backlight counts in 1/16 nit instead of whole nits |
| [install.sh](install.sh) | Makes every Gentoo `asahi-kernel` build apply it |
| [asahi-kernel.bashrc](asahi-kernel.bashrc) | The Portage hook that install.sh puts in place |

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

## Applying it automatically on Gentoo

```sh
sudo sh kernel/install.sh
sudo emerge --oneshot sys-kernel/asahi-kernel   # now, or wait for the next update
```

and reboot. `install.sh` puts the patch in `/usr/local/share/utils-mydesktop`
and a Portage hook in `/etc/portage/env/sys-kernel/asahi-kernel`. From then
on every `asahi-kernel` build, kernel updates included, runs the hook after
the kernel's own patches: it applies the patch if it fits, skips it if the
source already has it, and otherwise warns and builds without it. A kernel
update therefore never fails because of the patch; a plain
`/etc/portage/patches` entry would stop the build instead, so `install.sh`
removes one if it finds it. It will not overwrite a hook file it did not
write. `sudo sh kernel/install.sh remove` undoes it.

If a build warns that the patch no longer fits, the backlight counts whole
nits again (and [autobrightd](../autobrightd) shows 1-nit steps) until the
patch is updated for the new `drivers/gpu/drm/apple/dcp_backlight.c`.

The patch changes a file licensed GPL-2.0-only OR MIT and is under the same
terms.
