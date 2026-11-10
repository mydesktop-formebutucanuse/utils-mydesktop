# selinux

Brings SELinux up on this Gentoo in permissive mode, one step at a time:
the kernel loads a policy and logs what the policy would refuse, and
refuses nothing. Enforcing mode is not a step here (see
[Not enforcing](#not-enforcing)).

| File | What it is |
|---|---|
| [install.sh](install.sh) | The steps: `status`, `profile`, `packages`, `boot`, `relabel`, `remove` |
| [make.defaults](make.defaults) | For the Portage profile: one policy store, `mcs` |
| [dracut.conf](dracut.conf) | Has the initramfs load the policy |
| [selinuxfs.start](selinuxfs.start) | Mounts selinuxfs at every boot, from `/etc/local.d` |

Written for Gentoo's `default/linux/arm64/23.0/desktop` profile,
asahi-kernel 7.2.14, dracut 112 with a UKI, s6-linux-init and s6-rc, and
the SELinux packages of November 2026 (userland 3.11, policy 2.20260616).

**Only part of this has run.** `status` ran on the real system. `profile`,
`boot` and `remove` ran on a copy of `/etc`, and Portage resolved the
packages and the world update with the new profile (`emerge --pretend`).
The package builds, the initramfs with the policy loader, a boot with
SELinux on and `relabel` have not run at all.

## How it was

- The kernel is built with SELinux (`CONFIG_SECURITY_SELINUX=y`) but does
  not turn it on: `CONFIG_LSM` is `landlock,yama,bpf`.
- No SELinux tools, libraries or policy are installed.
- The Portage profile has `USE=selinux` masked.
- Neither s6-linux-init nor s6-rc loads a policy or mounts selinuxfs, which
  OpenRC's and systemd's init do themselves.

## The steps

```sh
sh selinux/install.sh
```

says where things stand and what comes next, without root. Each step
stops with a message if the one before it is not done.

### 1. `sudo sh selinux/install.sh profile`

Makes `/etc/portage/make.profile` a directory instead of a link, with

- `parent`: the profile the link pointed to, then Gentoo's
  `features/selinux`. That one unmasks and sets `USE=selinux` (and forces
  `audit` and `caps`), and turns on Portage's own SELinux features.
  Gentoo's ready-made SELinux profile for arm64 is `hardened/selinux`,
  which would take the desktop profile's settings away.
- `make.defaults`: one policy store, `mcs`, instead of all four.
- `wmutils.orig`: the link it replaced, for `remove`.

`eselect profile` no longer manages the profile after this. Portage says
"SELinux module not found" until the next step is done.

### 2. `sudo sh selinux/install.sh packages`

As Gentoo's SELinux installation guide has it:

- `FEATURES="-selinux" emerge --oneshot sec-policy/selinux-base`, which
  brings the tools (libselinux, policycoreutils, checkpolicy, setools,
  audit) with it;
- `/etc/selinux/config`: `SELINUX=permissive`, `SELINUXTYPE=mcs`;
- `selinux-base-policy`, `selinux-policykit` and `selinux-dbus`, with
  Portage's SELinux features off. Without the last two the policy has no
  `file_contexts` and nothing can be labelled (Gentoo bug 891963).

`mcs` is Gentoo's default store. With selinux-base's `unconfined` flag,
which is on by default, users and everything without a policy of its own
run unconfined, as in `targeted`.

### 3. By hand: the world update

```sh
sudo emerge --ask --verbose --update --deep --newuse @world && sudo dispatch-conf
```

On 2026-11-10 that was 43 new packages, 29 of them policy modules, and 43
rebuilds for the `selinux` flag: glibc, coreutils, util-linux, pam, shadow,
elogind, dbus, polkit, sudo, openssh, Portage, dracut, NetworkManager,
PipeWire, xlibre-server, QEMU and LibreWolf among them. It takes hours.
Nothing changes at run time yet: the kernel still runs without SELinux.

### 4. `sudo sh selinux/install.sh boot`

- Adds `lsm=landlock,yama,selinux,bpf enforcing=0` to `/etc/kernel/cmdline`,
  the command line installkernel builds into the UKI. `lsm=` is the
  kernel's own list with selinux added. `enforcing=0` keeps SELinux
  permissive whatever `/etc/selinux/config` says, and lets the boot go on
  if the policy does not load.
- Installs `/etc/dracut.conf.d/wmutils-selinux.conf`, which adds
  dracut's `selinux` module to the initramfs: it loads the policy from the
  root filesystem before the real init starts. Without a policy the
  kernel's SELinux allows everything and logs nothing.
- Installs `/etc/local.d/wmutils-selinuxfs.start`, which mounts
  selinuxfs at every boot (s6-rc's `init-local` runs it). The initramfs
  unmounts its own, and without it the tools take SELinux for off.

It stops if `/etc/selinux/config` does not say `permissive`, and if the
command line has another `lsm=` or `enforcing=` already.

### 5. By hand: the initramfs, the UKI and a reboot

```sh
sudo emerge --config sys-kernel/asahi-kernel
```

This replaces `EFI/BOOT/BOOTAA64.EFI`, the only thing U-Boot boots; there
is no menu to pick the old one from. installkernel keeps the previous UKI
as `EFI/Linux/*-old.efi` on the ESP. Nothing here should stop a boot
(`enforcing=0`, and a policy that does not load is skipped), but this is
the step that has not been tried.

After the reboot, `sh selinux/install.sh` shows `on now: yes; policy
loaded: yes (permissive)`. The files have no labels yet, so the log is
full of denials.

### 6. `sudo sh selinux/install.sh relabel`

Labels every file, as the guide has it: first the directories that mounts
cover (`/dev`, `/proc`, `/run`, `/sys`, `/home`, `/boot/efi`, ...), through
a second mount of `/`; the swap file as `swapfile_t`; then
`restorecon -RF /`. Both Btrfs subvolumes are labelled; the vfat ESP and
FUSE mounts cannot hold labels and are skipped.

### Looking at what it would refuse

No auditd runs under s6, so the denials go to the kernel log:

```sh
sudo dmesg | grep avc:
```

## Not enforcing

Permissive mode is where this stops, for a reason: Gentoo's policy knows
OpenRC and systemd, not s6.

- s6-linux-init and everything s6-rc starts have no types in the policy.
  The init most likely stays in the kernel's own domain, with every
  service it starts, and that domain is unconfined. So even in enforcing
  mode the policy would confine little of this system, and what it did
  refuse would be things nobody has tried here.
- `/run` (mounted by s6-linux-init) and the other tmpfs mounts get the
  default tmpfs label, not the ones the guide sets through `/etc/fstab`.
- Logins get their context from pam_selinux; whether that works from s6's
  gettys shows in the log first.

Before enforcing: a log without denials for the things the system does
every day, a policy module for s6, and a way back that does not need the
running system, since the command line is inside the UKI.

## Undoing it

```sh
sudo sh selinux/install.sh remove
```

puts the profile link back, takes `lsm=` and `enforcing=0` out of
`/etc/kernel/cmdline`, and removes the dracut drop-in and the selinuxfs
script. Then `emerge --config sys-kernel/asahi-kernel` and a reboot for a
kernel without SELinux, `emerge --ask --update --deep --newuse @world` to
build the packages without the flag again, and `emerge --ask --depclean`
for the tools and policies. The labels stay on the files, where nothing
reads them.
