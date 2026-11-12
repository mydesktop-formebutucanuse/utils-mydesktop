# kernel

Patches for the Asahi Linux kernel: the panel backlight without visible
steps, the PMP booted for lower idle power, and the ProMotion display's
variable refresh rate (VRR) on. Plus appledrm options: VRR, and the full
panel height around the notch. And a smaller kernel config, without the
drivers this MacBook cannot use, so that a kernel build takes less time.

| File | What it is |
|---|---|
| [0001-drm-apple-backlight-1-64-nit.patch](0001-drm-apple-backlight-1-64-nit.patch) | The panel backlight counts in 1/64 nit instead of whole nits |
| [0002-drm-apple-backlight-without-blocking.patch](0002-drm-apple-backlight-without-blocking.patch) | A brightness write returns at once, and the brightness can change about 60 times a second instead of 21 |
| [0003-arm64-dts-apple-j314s-boot-the-pmp.patch](0003-arm64-dts-apple-j314s-boot-the-pmp.patch) | Boots the Power Management Processor on the 14-inch M1 Pro (J314s) |
| [0004-drm-apple-adaptive-sync-promotion.patch](0004-drm-apple-adaptive-sync-promotion.patch) | VRR for the ProMotion panel and Adaptive Sync monitors, behind `appledrm.force_vrr` |
| [appledrm.conf](appledrm.conf) | Sets `appledrm.force_vrr=1` and `appledrm.show_notch=1` |
| [trim.config](trim.config) | A config fragment that turns off drivers for hardware this MacBook cannot take, kernel self-tests, and server parts |
| [required.txt](required.txt) | The options this Mac's own drivers need; a build stops before compiling if one is off |
| [install.sh](install.sh) | Makes every Gentoo `asahi-kernel` build apply the patches and trim.config and check required.txt, and installs appledrm.conf |
| [asahi-kernel.bashrc](asahi-kernel.bashrc) | The Portage hook that install.sh puts in place |

Each patch works without the others. The backlight patches change only
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

## PMP

The Power Management Processor (PMP) is a coprocessor that takes power
reports from the rest of the SoC and powers down idle parts of it. The
Asahi kernel builds its driver (`pmp.ko`) but leaves the device off in the
devicetree until the driver is upstream. The [Asahi Linux 7.0 progress
report](https://asahilinux.org/2026/04/progress-report-7-0/) measured about
half a watt, some 20 %, less at idle on a 14-inch M1 Pro with it on, and
says how to opt in: define `APPLE_USE_PMP` in the board's DTS.

The patch does that for `t6000-j314s.dts` (14-inch M1 Pro, the only board
it changes) and enables the `pmp` node. `APPLE_USE_PMP` routes the power
domains of the display controllers (DCP, DCPext), the camera ISP and the
video decoder through the PMP's report entries. The compiled DTB differs
from the stock one only there and in the node's status.

After booting a kernel with it, `lsmod | grep '^pmp'` lists the driver and
`/sys/firmware/devicetree/base/soc/pmp@28e700000/status` reads `okay`. If
it misbehaves (display, camera or video decoding stop powering up, or
`dmesg` shows `PMP firmware crashed`), boot with `modprobe.blacklist=pmp`
and remove the patch.

## VRR (ProMotion, Adaptive Sync)

A backport of James Calligeros's [Adaptive Sync/ProMotion
work](https://github.com/AsahiLinux/linux/pull/477) (seven commits on
`chadmed/linux` `dcp/vrr`, not yet merged into the Asahi kernel), adjusted
in one place for 7.2.14. DCP does VRR itself: the driver sets a minimum
refresh rate in the adaptive sync parameter and sets the mode again, and the
refresh rate then follows the frames' swap timestamps. The built-in panel
of the 14- and 16-inch MacBook Pros runs anywhere from 24 to 120 Hz;
external monitors use the range in their EDID.

It is off unless `appledrm.force_vrr=1`, which [appledrm.conf](appledrm.conf)
sets. KMS and the compositor are not told (`vrr_capable` stays 0), because
KMS does not allow the mode set DCP needs to switch it; the X server and
Window Maker need nothing. Upstream calls it experimental and reports that
mpv's `gpu` and `gpu-next` outputs can halt the GPU in fullscreen on the
built-in display. To turn it off without rebuilding, set `force_vrr=0` in
`/etc/modprobe.d/wmutils-appledrm.conf` and regenerate the
initramfs (`emerge --config sys-kernel/asahi-kernel`), or boot once with
`appledrm.force_vrr=0`.

`cat /sys/module/appledrm/parameters/force_vrr` shows `Y` when it is on.

## The notch

By default appledrm hides the strip on both sides of the camera notch and
gives X a 16:10 screen below it (3024x1890 on the 14-inch MacBook Pro).
`appledrm.show_notch=1`, also in [appledrm.conf](appledrm.conf), gives X
the whole panel instead (3024x1964): the top 74 pixels are then usable, and
the notch covers the middle of them. The driver reads the option only when
it starts, so it needs no patch but takes a reboot with the new initramfs.

Windows do not know about the notch: anything placed at the very top, a
maximised window's title bar for one, goes partly under it. A dock window
that fills the strip and reserves it with a strut
keeps them below it. Window Maker's own icons and clip are not in the
middle of the top edge and are not covered.

`cat /sys/module/appledrm/parameters/show_notch` shows `Y`, and `xrandr`
the taller mode, when it is on.

## A smaller config

The `asahi-kernel` ebuild starts from Fedora's config, which is made for
every arm64 machine Fedora runs on: 4767 options build modules (4946
files) and 2933 are compiled in. Most of them are for hardware a MacBook
cannot have. [trim.config](trim.config) turns those off, which leaves 2018
options building modules (2156 files) and 2565 compiled in. Compiling the
kernel and its modules (`make -j8 all`, as the ebuild does) took 11
minutes on this M1 Pro; with the stock config it had not finished after
17, having compiled half again as many files and linked no module yet.
Those numbers are 7.2.14's. In 7.3.3 the stock config has 4816 and 2960,
and the list leaves 2013 and 2587.

It is a config fragment: the ebuild merges every `*.config` file in
`/etc/kernel/config.d` last, and the kernel's `olddefconfig` then drops
whatever depended on what the fragment turns off. It only turns things
off: nothing is on that is off in the stock config. What goes:

- Whole menus for hardware that cannot be connected to a 14-inch MacBook
  Pro: PCI Ethernet, InfiniBand and RDMA, SATA controllers, PCI sound
  cards and HD Audio, SoundWire and SLIMbus, PCI capture cards, camera
  sensors on boards, FPGAs, CoreSight, raw NAND flash, the staging
  drivers, and the Chromebook, Surface and Mellanox platform drivers. This
  kernel tunnels USB, DisplayPort and networking over Thunderbolt but not
  PCIe, so a PCI chip in a dock or enclosure cannot reach the Mac either.
- TV and radio tuners and analog video capture, USB ones included.
- Kernel self-tests: KUnit, the torture tests, null_blk, netdevsim.
- Cluster and server parts with nothing installed to use them: the SCSI
  target (and with it vhost-scsi), GFS2, DLM, Ceph, DRBD, the NFS server,
  RDMA over Ethernet.
- One by one, drivers for chips that only sit on circuit boards, on I2C,
  SPI or PCI or inside other SoCs: SCSI, SAS, RAID and Fibre Channel
  cards, hardware monitors, regulators and PMICs, real-time clocks, GPIO
  expanders, LED controllers, IIO sensors, touchscreens and keypads, audio
  codecs, video decoders, GPUs, display panels and bridges, watchdogs,
  flash chips, crypto engines, Wi-Fi chips, CAN controllers, TPMs, and
  the calls into the firmware of Qualcomm SoCs.

What stays:

- Every driver this Mac uses. [required.txt](required.txt) lists their
  options: those of every module it loads, and of the driver for every
  device it has.
- Everything that can come in over USB-C or Bluetooth, except the tuners
  and capture devices above: USB Ethernet and Wi-Fi adapters, storage,
  serial adapters, audio interfaces, webcams, DisplayLink and other USB
  displays, keyboards, mice, game controllers and other HID devices, CAN
  adapters, NFC readers.
- Generic software other than the server parts above: file systems,
  network protocols, netfilter, KVM, vhost, virtio (the guest drivers
  too), md and dm, iSCSI over TCP, UBI and UBIFS (with block2mtd and
  mtdram for flash images). Containers, virtual machines, VPNs and disks
  from other machines work as before.
- Mice, tablets, joysticks and CAN adapters with a serial port, which
  `inputattach` or `ldattach` binds to a USB-serial adapter. The serial
  drivers that go bind only to a device named in a board's device tree.

### How it was checked

The list was generated from the kernel's Kconfig files and the stock
config, then merged the way the ebuild merges it, and the result compared
with the stock config:

- The 115 options in required.txt keep their stock values. They come from
  `lsmod` and from the modalias of every device in sysfs, resolved with
  `modprobe -R`.
- No USB or HID driver goes, except the tuners and capture devices above.
  The driver-by-driver part turns off only modules whose Kconfig entry
  sits with the board chips, and none that a kept driver, or a USB or HID
  driver, depends on.
- Each of the 368 options compiled into the stock kernel that end up off
  was looked at: helpers that only dropped drivers use (HDMI and ELD
  helpers, ATA and SCSI parts, PCI sound), tests, and workarounds for
  errata in CoreSight's trace buffer, which the M1 does not have. The boot
  path (Btrfs, the EFI stub, simpledrm, the Apple drivers, the initramfs)
  is unchanged.
- A trial build of 7.2.14_p1 with it (`make all`, as the ebuild compiles)
  finished without an error or a modpost warning. Every driver in
  required.txt was built, as a module or into the kernel, and so was every
  module loaded on this Mac except `ac97_bus` and `snd_compress`, which
  `snd_soc_core` only needed for other boards' sound drivers.

Rebuilding the kernel version that is running replaces its modules with
ones built for the smaller config, and the running kernel would load them
without noticing the difference (the stock config checks neither module
signatures nor symbol versions): reboot right after that build, before
plugging in anything new.

### When a new kernel comes

A newer kernel's new drivers build until the list is refreshed, and the
names it no longer has are ignored. Two things can go wrong.

A new driver can call a helper that its own Kconfig entry does not select,
and that only drivers on the list do. The stock config hides the mistake;
with the list the build stops at its end, in modpost. That was 7.3.3_p2:

```
ERROR: modpost: drivers/gpu/drm/panel/panel-chipone-icna35xx.ko: symbol 'drm_dsc_pps_payload_pack' undefined!
ERROR: modpost: drivers/gpu/drm/panel/panel-novatek-nt36536.ko: symbol 'drm_dsc_pps_payload_pack' undefined!
```

Two panel drivers new in 7.3, for a handheld and a tablet, use the DSC
helper (`DRM_DISPLAY_DSC_HELPER`) without selecting it, and every driver
that does select it is on the list. Such a driver joins the list, as
these two have; then `sudo sh kernel/install.sh` and the build again.

The other is a driver this Mac needs coming to depend on something the
list turns off, which `olddefconfig` would drop without a word. So once
the config is final, the hook checks it against required.txt, before
anything is compiled, and stops the build if an option is off:

```
 * wmutils: the kernel config leaves out what this Mac needs:
 *   CONFIG_DRM_APPLE (appledrm)
```

The option's Kconfig entry says what it depends on (Portage leaves the
source under `/var/tmp/portage` after a failed build); delete the line in
trim.config that turns that off, and run `sudo sh kernel/install.sh`
again. To build anyway:

```sh
sudo env WMUTILS_UNCHECKED=1 emerge --oneshot sys-kernel/asahi-kernel
```

An option in required.txt that the new kernel does not have at all
(renamed, say) only gets a warning: check that its module still builds,
and update required.txt.

### Refreshing the list

The list was made for 7.2.14_p1 and refreshed for 7.3.3_p2. A refresh
starts from the config of the old kernel and of the new one, each put
together as the ebuild does it (Fedora's config, the Gentoo and Asahi
fragments, the list, `olddefconfig`); for 7.2.14 that comes out as the
running kernel's `/proc/config.gz`, option for option.

- Every option that is on in the new kernel with the list, and was not in
  the old one, is looked at. For 7.3.3 that was 83. 30 of them went on the
  list, by the rules above, and with the two panels that did not build
  the list has 32 new lines; they and what depends on them turn 46 of the
  83 off:
  - chips on I2C and SPI: sensors and converters, two chargers, an LED and
    a backlight controller, a haptic driver, three audio codecs;
  - five more display panels;
  - a Wi-Fi chip on SDIO and one on PCI, and a RAID card;
  - the calls into the firmware of Qualcomm SoCs, which 7.3 offers on
    every arm64 kernel and Fedora compiles in. In 7.2 only Qualcomm's own
    drivers turned them on.
- The other 37 stay: two HID drivers, two Ethernet PHYs and the library
  for MCTP over USB, which a USB device can need; generic parts of the
  kernel (crypto, memory management, IOMMU page tables, Rust bindings,
  options of Landlock, AppArmor, EROFS and the like); two workarounds for
  errata of other CPUs, like the ones already there; and a serial card on
  PCIe, as the list leaves serial ports alone.
- What the list turns off without naming it, because it depends on
  something named, is compared between the two kernels. 7.3.3 has 26 such
  options more: tests, PCI Ethernet, HD Audio and SoundWire parts,
  hardware monitors and camera sensors, whose menus are on the list, and
  the USB audio offload to Qualcomm's sound DSP. Of the options compiled
  into the stock kernel, 373 end up off: the 368 of 7.2.14 and five of
  the Qualcomm firmware calls.
- Nothing is on that is off in the stock config, and the 115 options in
  required.txt keep their stock values.
- This directory's four patches fit 7.3.3_p2 as they are.

### Turning something back on

Delete its line from trim.config and run `sudo sh kernel/install.sh`
again; the next build has it. The option gets its stock value back, and so
does everything under it: without `# CONFIG_ETHERNET is not set` all the
PCI Ethernet drivers return. The likely ones:

- `CONFIG_ETHERNET` and `CONFIG_ATA`, once the Asahi kernel tunnels PCIe
  over Thunderbolt, for docks and enclosures with a PCI Ethernet or SATA
  chip. NVMe enclosures need nothing: NVMe stays.
- `CONFIG_MEDIA_ANALOG_TV_SUPPORT`, `CONFIG_MEDIA_DIGITAL_TV_SUPPORT` and
  `CONFIG_MEDIA_RADIO_SUPPORT`, for a USB TV or radio stick.
- `CONFIG_TARGET_CORE`, for vhost-scsi disks in virtual machines or an
  iSCSI target (with targetcli).

Without `/etc/kernel/config.d/wmutils-trim.config` the build has
the full stock config again, until install.sh puts it back.

## Applying them automatically on Gentoo

```sh
sudo sh kernel/install.sh
sudo emerge --oneshot sys-kernel/asahi-kernel   # now, or wait for the next update
```

and reboot. `install.sh` puts the patches and required.txt in
`/usr/local/share/wmutils` (removing any patch there that left the
series), a Portage hook in `/etc/portage/env/sys-kernel/asahi-kernel`,
trim.config in `/etc/kernel/config.d/wmutils-trim.config`, and
appledrm.conf in `/etc/modprobe.d/wmutils-appledrm.conf`. The
kernel build's dracut run copies that into the initramfs, where appledrm
is loaded, and its m1n1 hook puts the new DTB in the boot loader.
From then on every `asahi-kernel` build, kernel updates included, runs the
hook after the kernel's own patches. For each patch in turn it applies the
patch if it fits, skips it if the source already has it, and otherwise
warns and builds without it. A kernel update therefore never fails because
of a patch; a plain `/etc/portage/patches` entry would stop the build
instead, so `install.sh` removes one if it finds it. The ebuild merges
trim.config into the config, and once it is final the hook checks it
against required.txt (see [When a new kernel comes](#when-a-new-kernel-comes)).
`install.sh` will not overwrite a hook file it did not write. Run it again
after anything in this directory changes. `sudo sh kernel/install.sh
remove` undoes it all.

If a build warns that a patch no longer fits, that patch is missing until it
is updated: without 0001 the backlight counts whole nits (and
[autobrightd](../autobrightd) shows 1-nit steps), without 0002 brightness
changes are back to about 21 updates a second, without 0003 the PMP stays
off, and without 0004 the display runs at a fixed rate (appledrm then
ignores `force_vrr` with a warning in `dmesg`). A kernel that already has
a patch's change is detected and the patch skipped, so once the Asahi
kernel ships VRR or turns the PMP on, nothing breaks.

The patches change files licensed GPL-2.0-only OR MIT (the devicetree:
GPL-2.0+ OR MIT) and are under the same terms.
