# selinux

Brings SELinux up on this Gentoo in permissive mode, one step at a time:
the kernel loads a policy, s6's init, the services and the logins run in
the policy's domains, and the kernel logs what the policy would refuse and
refuses nothing. Enforcing mode is not a step here (see
[Not enforcing](#not-enforcing)).

| File | What it is |
|---|---|
| [install.sh](install.sh) | The steps: `status`, `profile`, `packages`, `boot`, `relabel`, `s6`, `s6-undo`, `remove` |
| [make.defaults](make.defaults) | For the Portage profile: one policy store, `mcs` |
| [dracut.conf](dracut.conf) | Has the initramfs load the policy |
| [selinuxfs.start](selinuxfs.start) | Mounts selinuxfs at every boot, from `/etc/local.d` |
| [rc.selinux](rc.selinux) | Run first by s6-linux-init's `rc.init`: mounts selinuxfs and labels `/run` and `/dev` before any service starts |

Written for Gentoo's `default/linux/arm64/23.0/desktop` profile,
asahi-kernel 7.2.14, dracut 112 with a UKI, s6-linux-init and s6-rc, and
the SELinux packages of November 2026 (userland 3.11, policy 2.20260616).

**Steps 1 to 6 have run on this machine**, on 2026-11-10 and 11: it boots
with SELinux on, permissive, with the policy loaded and every file
labelled. The world update ran after the reboot here, not before `boot`;
both orders work. `remove` has only run on a copy of `/etc`.

**Step 7, the one for s6, has not run on it yet**, nor been through a
boot. It was tried without root, on copies; the step says how.

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

On 2026-11-11 that was 67 packages: 24 more policy modules and 43 rebuilds
for the `selinux` flag: glibc, coreutils, util-linux, pam, shadow, elogind,
dbus, polkit, sudo, openssh, Portage, dracut, NetworkManager, PipeWire,
xlibre-server, QEMU and LibreWolf among them. It took an hour on the M1
Pro. Nothing changes at run time yet: the kernel still runs without
SELinux.

It can also run after the reboot of step 5, as it did here, but before
`relabel`. Until coreutils is rebuilt `ls -Z` shows no labels, and `status`
and `relabel` say so. With SELinux on, Portage says `!!! Failed to set new
SELinux execution context` in every phase of every package and builds
them all the same (step 7 says why).

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
as `EFI/Linux/*-old.efi` on the ESP. Nothing here stops a boot
(`enforcing=0`, and a policy that does not load is skipped), and it did
boot.

After the reboot, `sh selinux/install.sh` shows `on now: yes; policy
loaded: yes (permissive)`. The files have no labels yet: the kernel takes
every one for `unlabeled_t`.

### 6. `sudo sh selinux/install.sh relabel`

Labels every file, as the guide has it: first the directories that mounts
cover (`/dev`, `/proc`, `/run`, `/sys`, `/home`, `/boot/efi`, ...), through
a second mount of `/`; the swap file as `swapfile_t`; then
`restorecon -RF /`. Both Btrfs subvolumes are labelled; the vfat ESP
cannot hold labels and is skipped.

- A user's FUSE mounts (rclone, the document portal in
  `/run/user/1000/doc`) are closed to root. restorecon says `Could not
  stat ...: Permission denied` for each, labels everything else and fails
  all the same. The step lets that message pass for the FUSE mounts root
  cannot enter, names them at the end, and stops on any other error.
- Files in `/tmp` and `/var/tmp` keep what they had: the policy gives
  them no label (`<<none>>`). `/tmp` is emptied at the next boot.
- It stops before it starts if `ls -Z` shows no labels, which is a
  coreutils from before the world update.

On 2026-11-11 no file outside `/tmp` and `/var/tmp` was `unlabeled_t`
afterwards, out of about a million.

### 7. `sudo sh selinux/install.sh s6`

After step 6 the policy is loaded and confines nothing. What a boot with
every file labelled showed on 2026-11-11:

- s6-linux-init and everything s6-rc starts have no types in the policy:
  the init script is `etc_t` and `s6-svscan` is `bin_t`, so nothing takes
  the init out of the kernel's own domain. The init, every service, the
  gettys, the logins and the X session ran as
  `system_u:system_r:kernel_t:s0`, which is unconfined; only udev had a
  type of its own.
- Logins got no context. pam_selinux is in `/etc/pam.d/system-login`, but
  the policy has default contexts for a login from `local_login_t`,
  `sshd_t`, `xdm_t` or `init_t`, and none for one from `kernel_t`. `id -Z`
  said `kernel_t` too, and Portage cannot switch to its own context from
  there, which is the message of step 3.
- `/run`, which s6-linux-init mounts, was `tmpfs_t`, and every device node
  `device_t`: the initramfs relabels `/dev` only after it has unmounted
  selinuxfs, which does nothing, and udev started before the script of
  step 4 mounts selinuxfs again, so it took SELinux for off.
- The boot and a few minutes of use left 13 `avc:` lines in the kernel
  log.

The policy has no types for s6. This step gives s6's files the types the
policy has for the init and the init scripts it knows, as local file
contexts (`semanage fcontext`), which a later `relabel` and Portage keep:

| File | Type | What the policy does with it |
|---|---|---|
| `/etc/s6-linux-init/current/bin/init`, where `/sbin/init` leads | `init_exec_t` | The kernel's domain becomes `init_t`: s6-linux-init, then s6-svscan as process 1 |
| `/usr/bin/s6-supervise` and `/etc/s6-linux-init/current/scripts/*` | `initrc_exec_t` | `init_t` becomes `initrc_t`, the domain of init scripts: the supervisors and the run scripts, `rc.init`, s6-rc. From there the policy has a domain for every daemon it knows (dbus, NetworkManager, udev, chrony, elogind, bluetooth, ...) and for getty; from getty, login is `local_login_t`, and pam_selinux gives the user `unconfined_t` |
| `/run/service` and `/run/uncaught-logs` | `initrc_runtime_t` | What `initrc_t` makes in `/run` anyway; s6-linux-init copies these there before anything can label them |

What has no policy of its own (`s6-log`, rclone, speakersafetyd) stays in
`initrc_t`, which is unconfined, like the user's `unconfined_t`.

It also installs [rc.selinux](rc.selinux) as
`/etc/s6-linux-init/current/scripts/wmutils-selinux` and adds one line to
`rc.init`, as its second, that runs it before s6-rc starts anything:

- It mounts selinuxfs. udev and the daemons built with SELinux label what
  they make only if they find it when they start. The script of step 4
  stays; it finds selinuxfs mounted and does nothing.
- `restorecon -RF /run /dev`: `/run` becomes `var_run_t`, from which the
  policy gives what each service makes there its type, and the device
  nodes from before udev get theirs (`tty_device_t`,
  `fixed_disk_device_t`, `sound_device_t`, ...).

The line cannot stop `rc.init`, and the script always ends well; on a boot
without SELinux or without a policy it does nothing.

SELinux stays permissive, so nothing is refused that was allowed before;
what changes is which domain the log names, and that the log now says what
the policy would refuse the services and the logins. Nothing changes until
the next boot. After it:

```sh
sh selinux/install.sh
ps -eo label,args
id -Z
```

`status` says which domain the init runs in, `ps` the domain of every
process, and `id -Z` should say `unconfined_u:unconfined_r:unconfined_t`.

If the boot does not get as far as a login: s6-linux-init's early getty on
tty6 does not depend on `rc.init`, and from there

```sh
sudo sh selinux/install.sh s6-undo
```

takes the line, the script and the file contexts away again and puts the
old labels back.

A new base directory from `s6-linux-init-maker` has neither the line nor
the script; `status` says so, and the step can run again.

What was tried, without root, on 2026-11-11:

- `s6`, `s6` again and `s6-undo` on copies of `/etc/s6-linux-init` and
  `/etc/selinux` in a user namespace, with a stand-in for `semanage` and
  the real `restorecon`: the labels as in the table, `rc.init` different
  by its second line only, and after the undo the same as before, byte
  for byte. `remove` after `s6`, on a copy of `/etc`, the same way.
- `restorecon -n` over the real `/run` and `/dev` with those contexts.
- The boot script with stand-ins for `mount` and `restorecon`, with
  selinuxfs there, missing, and failing to mount; and the line in
  `rc.init` with the script missing, failing and not executable.
- The domains, with a private `s6-svscan` over copies that carry the
  labels: `init_t` from the init script, `initrc_t` for s6-supervise and
  the run scripts, then `getty_t` and `local_login_t`, and
  `system_dbusd_t`, `NetworkManager_t`, `udev_t`, `chronyd_t`,
  `syslogd_t`, `bluetooth_t` and `systemd_logind_t` for copies labelled
  as those daemons are. From `local_login_t` the policy's default context
  for the user is `unconfined_t`.

Not tried: the real `semanage` with these contexts, the step as root, and
a boot.

### Looking at what it would refuse

No auditd runs under s6, so the denials go to the kernel log:

```sh
sudo dmesg | grep avc:
```

## Not enforcing

Permissive mode is where this stops, for a reason: Gentoo's policy knows
OpenRC and systemd, not s6. Step 7 borrows their types for s6, and no boot
has shown yet what the policy refuses of what s6 does under them and an
init script does not: the supervisors' pipes and fifos, the control
sockets in `/run/service`, s6-log and the catch-all logger that every
daemon's output goes through.

Before enforcing: step 7 through a boot; a kernel log without denials for
the things the system does every day, with a policy module for what s6
needs and the borrowed types do not allow; and a way back that does not
need the running system, since the command line is inside the UKI.

## Undoing it

```sh
sudo sh selinux/install.sh remove
```

undoes step 7 as `s6-undo` does, puts the profile link back, takes `lsm=`
and `enforcing=0` out of `/etc/kernel/cmdline`, and removes the dracut
drop-in and the selinuxfs script. Then
`emerge --config sys-kernel/asahi-kernel` and a reboot for a kernel
without SELinux, `emerge --ask --update --deep --newuse @world` to build
the packages without the flag again, and `emerge --ask --depclean` for the
tools and policies. The labels stay on the files, where nothing reads
them.
