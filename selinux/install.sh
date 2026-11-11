#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Bring SELinux up on this Gentoo in permissive mode, one step at a time:
# the kernel loads a policy and logs what the policy would refuse, and
# refuses nothing. Each step when the one before is done:
#   sh selinux/install.sh [status]        where things stand and what comes next
#   sudo sh selinux/install.sh profile    the Portage profile: the present one plus features/selinux
#   sudo sh selinux/install.sh packages   the SELinux tools and the base policy; /etc/selinux/config
#   sudo sh selinux/install.sh boot       the kernel command line, the initramfs' policy loader, selinuxfs
#   sudo sh selinux/install.sh relabel    after the reboot: label every file
#   sudo sh selinux/install.sh s6         s6's init, services and logins in the policy's domains
#   sudo sh selinux/install.sh s6-undo    undo s6
#   sudo sh selinux/install.sh remove     undo profile, boot and s6
# Three things stay by hand, and status says when: the world update after
# packages, the new initramfs and the reboot after boot, and the reboot
# after s6.
# Enforcing mode is not a step here: see selinux/README.md.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
profile=/etc/portage/make.profile
# In our profile directory: the link it replaced.
orig=$profile/wmutils.orig
config=/etc/selinux/config
cmdline=/etc/kernel/cmdline
dracutconf=/etc/dracut.conf.d/wmutils-selinux.conf
start=/etc/local.d/wmutils-selinuxfs.start
# The project was utils-mydesktop until November 2026: the same three under
# that name, which the first step run as root renames.
oldorig=$profile/utils-mydesktop.orig
olddracutconf=/etc/dracut.conf.d/utils-mydesktop-selinux.conf
oldstart=/etc/local.d/utils-mydesktop-selinuxfs.start
# The policy store: Gentoo's default. selinux/make.defaults names it too.
type=mcs
contexts=/etc/selinux/$type/contexts/files/file_contexts
# What ends our line in s6-linux-init's rc.init.
mark='# wmutils: selinux'
action=${1:-status}

die() {
	echo "$*" >&2
	exit 1
}

need_root() {
	[ "$(id -u)" -eq 0 ] || die "Run as root: sudo sh $0 $action"
	adopt_old
}

# Gives what was installed under the old name its new one.
adopt_old() {
	if [ ! -L "$profile" ] && [ -f "$oldorig" ]; then
		mv "$oldorig" "$orig"
		sed -i 's/utils-mydesktop/wmutils/g' "$profile/make.defaults"
		echo "renamed $oldorig to $orig"
	fi
	if [ -e "$olddracutconf" ]; then
		mv "$olddracutconf" "$dracutconf"
		sed -i 's/utils-mydesktop/wmutils/g' "$dracutconf"
		echo "renamed $olddracutconf to $dracutconf"
	fi
	if [ -e "$oldstart" ]; then
		mv "$oldstart" "$start"
		sed -i 's/utils-mydesktop/wmutils/g' "$start"
		echo "renamed $oldstart to $start"
	fi
}

yesno() {
	if "$@"; then echo yes; else echo no; fi
}

# The profile directory is ours if it holds the link it replaced.
ours() {
	[ ! -L "$profile" ] && { [ -f "$orig" ] || [ -f "$oldorig" ]; }
}

has_selinux_use() {
	portageq envvar USE 2>/dev/null | tr ' ' '\n' | grep -qx selinux
}

kernel_built() {
	zcat /proc/config.gz 2>/dev/null | grep -qx 'CONFIG_SECURITY_SELINUX=y'
}

# The kernel's classes are in selinuxfs once a policy is loaded.
policy_loaded() {
	[ -d /sys/fs/selinux/class/file ]
}

# The label of /, as ls shows it: nothing if ls cannot show labels. With a
# policy loaded every file has one, so nothing then means a coreutils
# built without selinux: the world update has not run.
root_label() {
	l=$(ls -Zd / 2>/dev/null | cut -d' ' -f1)
	case "$l" in
	*:*) echo "$l" ;;
	esac
}

world_update() {
	echo "  sudo emerge --ask --verbose --update --deep --newuse @world && sudo dispatch-conf"
}

# The type in the label of the file $1, as ls shows it.
type_of() {
	ls -Zd "$1" 2>/dev/null | cut -d' ' -f1 | cut -d: -f3
}

# s6-linux-init's files, from where /sbin/init leads: its init script and,
# beside it, the scripts, of which rc.init starts the services; and
# s6-supervise, which runs every service. Fails if the init is not
# s6-linux-init's.
s6_paths() {
	init=$(readlink -f /sbin/init) || return 1
	case "$init" in
	*/bin/init) ;;
	*) return 1 ;;
	esac
	grep -q s6-linux-init "$init" 2>/dev/null || return 1
	scripts=${init%/bin/init}/scripts
	rcinit=$scripts/rc.init
	hook=$scripts/wmutils-selinux
	[ -f "$rcinit" ] || return 1
	supervise=$(command -v s6-supervise) || return 1
	supervise=$(readlink -f "$supervise")
}

# The path $1 as a pattern for a file context.
regex() {
	printf '%s\n' "$1" | sed 's/\./\\./g'
}

# The file contexts of the s6 step, one a line: type, kind of file as
# semanage has it (f a regular file, a any), pattern. The policy has no
# types for s6, so its files borrow those of the init and the init scripts
# the policy knows:
# - the init script init_exec_t, for which the kernel's domain changes to
#   init_t;
# - s6-supervise and the rc scripts initrc_exec_t, for which init_t changes
#   to initrc_t, the domain of init scripts. From there the policy has a
#   change for every daemon it knows, and for getty, from which a login
#   gets the user's context;
# - what s6-linux-init copies to /run before anything can label it
#   initrc_runtime_t, which is also what initrc_t makes there later.
s6_contexts() {
	printf '%s\n' \
		"init_exec_t f $(regex "$init")" \
		"initrc_exec_t f $(regex "$supervise")" \
		"initrc_exec_t f $(regex "$scripts")/.*" \
		"initrc_runtime_t a /run/service(/.*)?" \
		"initrc_runtime_t a /run/uncaught-logs(/.*)?"
}

# Is the file context of type $1, kind $2 and pattern $3 among the local
# ones?
has_context() {
	T=$1 K=$2 P=$3 awk '
		$1 == ENVIRON["P"] {
			c = $2
			if (ENVIRON["K"] == "f") c = ($2 == "--") ? $3 : ""
			if (split(c, part, ":") >= 3 && part[3] == ENVIRON["T"]) found = 1
		}
		END { exit !found }' "$contexts.local" 2>/dev/null
}

# Do the init script, s6-supervise and rc.init have the types of the s6 step?
s6_labelled() {
	[ "$(type_of "$init")" = init_exec_t ] &&
		[ "$(type_of "$supervise")" = initrc_exec_t ] &&
		[ "$(type_of "$rcinit")" = initrc_exec_t ]
}

# Does rc.init run our script?
s6_hooked() {
	[ -x "$hook" ] && grep -q " $mark\$" "$rcinit"
}

# Undoes the s6 step: our line in rc.init, our script, the file contexts
# and the labels they gave.
s6_undo() {
	s6_paths || return 0
	if grep -q " $mark\$" "$rcinit"; then
		new=$rcinit.wmutils-new
		cp -p "$rcinit" "$new"
		sed "/ $mark\$/d" "$rcinit" >"$new"
		mv "$new" "$rcinit"
		echo "took our line out of $rcinit"
	fi
	if [ -e "$hook" ]; then
		rm -f "$hook"
		echo "removed $hook"
	fi
	if command -v semanage >/dev/null; then
		s6_contexts | while read -r t k p; do
			has_context "$t" "$k" "$p" || continue
			semanage fcontext --delete --ftype "$k" "$p" </dev/null
			echo "file context: $p is the policy's own again"
		done
	fi
	if command -v restorecon >/dev/null && policy_loaded; then
		restorecon -F "$init" "$supervise"
		restorecon -RF "$scripts"
	fi
}

# The kernel's own list of security modules with selinux in it, for lsm=:
# before bpf, which stays last, or at the end.
lsm_list() {
	list=$(zcat /proc/config.gz 2>/dev/null | sed -n 's/^CONFIG_LSM="\(.*\)"$/\1/p')
	[ -n "$list" ] || return 1
	case ",$list," in
	*,selinux,*) echo "$list" ;;
	*,bpf,) echo "${list%bpf}selinux,bpf" ;;
	*) echo "$list,selinux" ;;
	esac
}

# Is the word $1 on the kernel command line $2?
has_word() {
	case " $2 " in
	*" $1 "*) return 0 ;;
	esac
	return 1
}

case "$action" in
status)
	lsm=$(lsm_list) || lsm=
	file=$(cat "$cmdline" 2>/dev/null) || file=
	k_built=$(yesno kernel_built)
	k_on=$(yesno test -d /sys/fs/selinux)
	k_policy=$(yesno policy_loaded)
	mode=
	case "$(cat /sys/fs/selinux/enforce 2>/dev/null)" in
	0) mode=permissive ;;
	1) mode=enforcing ;;
	esac
	p_use=$(yesno has_selinux_use)
	pk_tools=$(yesno sh -c 'command -v load_policy >/dev/null && command -v restorecon >/dev/null')
	pk_policy=$(yesno test -f "$contexts")
	pk_type=$(sed -n 's/^SELINUXTYPE=//p' "$config" 2>/dev/null) || pk_type=
	pk_mode=$(sed -n 's/^SELINUX=//p' "$config" 2>/dev/null) || pk_mode=
	b_file=no
	if [ -n "$lsm" ] && has_word "lsm=$lsm" "$file" && has_word enforcing=0 "$file"; then
		b_file=yes
	fi
	b_dracut=$(yesno sh -c '[ -f "$1" ] || [ -f "$2" ]' sh "$dracutconf" "$olddracutconf")
	b_start=$(yesno sh -c '[ -x "$1" ] || [ -x "$2" ]' sh "$start" "$oldstart")
	l_root=$(root_label)
	s6=no
	s6_done=no
	pid1=
	if s6_paths; then
		s6=yes
		s6_l=$(yesno s6_labelled)
		s6_h=$(yesno s6_hooked)
		if [ "$s6_l" = yes ] && [ "$s6_h" = yes ]; then
			s6_done=yes
		fi
		pid1=$(tr -d '\0\n' </proc/1/attr/current 2>/dev/null) || pid1=
	fi
	echo "kernel   built with SELinux: $k_built; on now: $k_on; policy loaded: $k_policy${mode:+ ($mode)}"
	echo "          security modules now: $(cat /sys/kernel/security/lsm 2>/dev/null)"
	echo "profile   ours: $(yesno ours); USE has selinux: $p_use"
	echo "packages  tools: $pk_tools; $type policy: $pk_policy; $config: SELINUX=${pk_mode:-(none)} SELINUXTYPE=${pk_type:-(none)}"
	echo "boot      $cmdline has lsm=${lsm:-?} enforcing=0: $b_file; dracut loads the policy: $b_dracut; selinuxfs at boot: $b_start"
	echo "labels    /: ${l_root:-(ls shows none)}"
	if [ "$s6" = yes ]; then
		echo "s6        init, s6-supervise and rc.init have the policy's types: $s6_l; rc.init labels /run and /dev: $s6_h"
		echo "          init runs as: ${pid1:-(no label)}"
	fi
	if [ -e "$oldorig" ] || [ -e "$olddracutconf" ] || [ -e "$oldstart" ]; then
		echo "old name  some of it is installed as utils-mydesktop; the next step run as root renames it"
	fi
	echo
	if [ "$k_built" = no ]; then
		echo "Next: nothing. This kernel is not built with SELinux (CONFIG_SECURITY_SELINUX)."
	elif [ "$p_use" = no ]; then
		echo "Next: sudo sh $0 profile"
	elif [ "$pk_tools" = no ] || [ "$pk_policy" = no ] || [ "$pk_type" != "$type" ]; then
		echo "Next: sudo sh $0 packages"
	elif [ "$b_file" = no ] || [ "$b_dracut" = no ] || [ "$b_start" = no ]; then
		echo "Next: the world update, if it is not done:"
		world_update
		echo "then: sudo sh $0 boot"
	elif [ "$k_on" = no ]; then
		echo "Next: the initramfs and the UKI with the new command line, then a reboot:"
		echo "  sudo emerge --config sys-kernel/asahi-kernel"
	elif [ "$k_policy" = no ]; then
		echo "SELinux is on in the kernel, but no policy is loaded or selinuxfs is not mounted."
		echo "Was the initramfs made after the boot step? sudo emerge --config sys-kernel/asahi-kernel"
	elif [ -z "$l_root" ]; then
		echo "Next: the world update, which has not run: ls shows no labels, so"
		echo "coreutils is still built without selinux."
		world_update
		echo "then: sudo sh $0 relabel"
	elif [ "${l_root#*:root_t}" = "$l_root" ]; then
		echo "Next: sudo sh $0 relabel"
	elif [ "$s6" = yes ] && [ "$s6_done" = no ]; then
		echo "SELinux is on, $mode, and the files are labelled, but s6 starts everything"
		echo "in the kernel's own domain, where the policy confines nothing."
		echo "Next: sudo sh $0 s6"
	elif [ "$s6" = yes ] && [ "${pid1#*:init_t:}" = "$pid1" ]; then
		echo "SELinux is on, $mode, and the files are labelled."
		echo "Next: a reboot, after which init, the services and the logins run in the policy's domains."
	else
		echo "SELinux is on, $mode, and the files are labelled."
		if [ "$s6" = yes ]; then
			echo "init runs as init_t; which domain everything else runs in: ps -eo label,args"
		fi
		echo "What the policy would refuse: sudo dmesg | grep avc:"
	fi
	;;
profile)
	need_root
	gentoo=$(portageq get_repo_path / gentoo) || gentoo=
	feature=$gentoo/profiles/features/selinux
	[ -d "$feature" ] || die "There is no $feature."
	if [ -L "$profile" ]; then
		link=$(readlink "$profile")
	elif ours; then
		# Ours already: written again from the link it replaced.
		link=$(cat "$orig")
	else
		die "$profile is neither a link nor ours; add $feature to its parents by hand."
	fi
	base=$(cd "${profile%/*}" && readlink -f "$link") || base=
	[ -n "$base" ] && [ -d "$base" ] || die "$profile points to $link, which is not there."
	if [ -L "$profile" ] && has_selinux_use; then
		echo "$base is a SELinux profile already; left as it is."
		exit 0
	fi
	new=$profile
	if [ -L "$profile" ]; then
		new=$profile.wmutils
		mkdir "$new" || die "$new is in the way."
	fi
	printf '%s\n' "$link" >"$new/wmutils.orig"
	# The profile the system had, then SELinux on top of it.
	printf '%s\n%s\n' "$base" "$feature" >"$new/parent"
	install -m 644 "$here/make.defaults" "$new/make.defaults"
	if [ "$new" != "$profile" ]; then
		rm "$profile"
		mv "$new" "$profile"
	fi
	echo "installed $profile: $base and $feature"
	has_selinux_use || die "Portage does not show USE=selinux with it; undo with: sudo sh $0 remove"
	echo "eselect profile no longer manages the profile: it is a directory now."
	echo "Portage says 'SELinux module not found' until the packages step is done."
	echo "Next: sudo sh $0 packages"
	;;
packages)
	need_root
	has_selinux_use || die "USE has no selinux yet: sudo sh $0 profile"
	# Portage cannot label files or load policy yet, so its SELinux
	# features are off for these.
	FEATURES="-selinux" emerge --oneshot sec-policy/selinux-base
	[ -f "$config" ] || die "selinux-base did not install $config."
	sed -i -e 's/^SELINUX=.*/SELINUX=permissive/' -e "s/^SELINUXTYPE=.*/SELINUXTYPE=$type/" "$config"
	if ! grep -qx 'SELINUX=permissive' "$config" || ! grep -qx "SELINUXTYPE=$type" "$config"; then
		die "$config has no SELINUX= and SELINUXTYPE= lines to set; it needs SELINUX=permissive and SELINUXTYPE=$type."
	fi
	echo "$config: SELINUX=permissive, SELINUXTYPE=$type"
	# policykit and dbus too: without them the policy has no
	# file_contexts and nothing can be labelled (Gentoo bug 891963).
	FEATURES="-selinux -sesandbox" emerge --oneshot sec-policy/selinux-base-policy \
		sec-policy/selinux-policykit sec-policy/selinux-dbus
	echo "The SELinux tools and the base policy are installed."
	echo "Next, by hand, the packages whose USE flags the profile changed, and their policies:"
	echo "  emerge --ask --verbose --update --deep --newuse @world && dispatch-conf"
	echo "then: sudo sh $0 boot"
	;;
boot)
	need_root
	kernel_built || die "The running kernel is not built with SELinux (CONFIG_SECURITY_SELINUX)."
	command -v load_policy >/dev/null && command -v setenforce >/dev/null ||
		die "load_policy and setenforce are missing: sudo sh $0 packages"
	[ -f "$contexts" ] || die "There is no $type policy yet: sudo sh $0 packages"
	grep -qx 'SELINUX=permissive' "$config" ||
		die "$config does not say SELINUX=permissive; this script only sets up permissive mode."
	[ -d /usr/lib/dracut/modules.d ] || die "There is no dracut (sys-kernel/dracut)."
	[ -d "${start%/*}" ] || die "There is no ${start%/*}, whose *.start scripts s6-rc's init-local runs at boot."
	[ -f "$cmdline" ] || die "There is no $cmdline, from which installkernel takes the UKI's command line."
	lsm=$(lsm_list) || die "The kernel's CONFIG_LSM is not in /proc/config.gz."
	cur=$(cat "$cmdline")
	add=
	case " $cur " in
	*" lsm=$lsm "*) ;;
	*" lsm="*) die "$cmdline has another lsm= already; it needs lsm=$lsm." ;;
	*) add="$add lsm=$lsm" ;;
	esac
	# Whatever the config file says, and if the policy does not load:
	# permissive, and the boot goes on.
	case " $cur " in
	*" enforcing=0 "*) ;;
	*" enforcing="*) die "$cmdline has another enforcing= already; it needs enforcing=0." ;;
	*) add="$add enforcing=0" ;;
	esac
	if [ -n "$add" ]; then
		printf '%s%s\n' "$cur" "$add" >"$cmdline.new"
		mv "$cmdline.new" "$cmdline"
		echo "added to $cmdline:$add"
	fi
	install -D -m 644 "$here/dracut.conf" "$dracutconf"
	echo "installed $dracutconf"
	install -D -m 755 "$here/selinuxfs.start" "$start"
	echo "installed $start"
	echo "Next, by hand: the initramfs and the UKI with the new command line, then a reboot."
	echo "  emerge --config sys-kernel/asahi-kernel"
	echo "After the reboot: sh $0 status, then sudo sh $0 relabel"
	;;
relabel)
	need_root
	[ -d /sys/fs/selinux ] ||
		die "SELinux is not on in the running kernel: sh $0 status says what is missing."
	mountpoint -q /sys/fs/selinux || mount -t selinuxfs -o nosuid,noexec selinuxfs /sys/fs/selinux
	policy_loaded || die "No policy is loaded: sh $0 status says what is missing."
	[ -f "$contexts" ] || die "There is no $contexts."
	if [ -z "$(root_label)" ]; then
		echo "ls shows no labels: coreutils is still built without selinux. The world update first:" >&2
		world_update >&2
		exit 1
	fi
	# The directories that mounts cover: label them through a second
	# mount of / that has nothing mounted in it.
	mnt=$(mktemp -d)
	trap 'umount "$mnt" 2>/dev/null; rmdir "$mnt" 2>/dev/null' EXIT
	mount -o bind / "$mnt"
	findmnt -rno TARGET | while read -r t; do
		[ "$t" != / ] && [ "$t" != "$mnt" ] && [ -d "$mnt$t" ] || continue
		setfiles -r "$mnt" "$contexts" "$mnt$t" || echo "could not label the directory under $t" >&2
	done
	umount "$mnt"
	rmdir "$mnt"
	trap - EXIT
	echo "labelled the directories under the mount points"
	# A swap file needs a type of its own.
	sed 1d /proc/swaps | while read -r f kind rest; do
		[ "$kind" = file ] || continue
		semanage fcontext --add --seuser system_u --type swapfile_t --ftype f "$f" 2>/dev/null || true
		echo "swap file $f: swapfile_t"
	done
	# A FUSE mount of a user's is closed to root. restorecon says "Could
	# not stat" for it, labels everything else and fails all the same.
	# That message for such a mount is let pass, and nothing else.
	nl='
'
	closed=$(findmnt -lno FSTYPE,TARGET | while read -r fs t; do
		case "$fs" in
		fuse | fuse.*) [ -e "$t" ] || printf '%s\n' "$t" ;;
		esac
	done)
	expected=
	[ -z "$closed" ] ||
		expected=$(printf '%s\n' "$closed" | sed 's/.*/restorecon: Could not stat &: Permission denied./')
	echo "labelling every file; this takes a while"
	err=$(mktemp)
	trap 'rm -f "$err"' EXIT
	rc=0
	LC_ALL=C restorecon -RF -T 0 / 2>"$err" || rc=$?
	other=no
	while IFS= read -r line; do
		case "$nl$expected$nl" in
		*"$nl$line$nl"*) ;;
		*)
			printf '%s\n' "$line" >&2
			other=yes
			;;
		esac
	done <"$err"
	rm -f "$err"
	trap - EXIT
	if [ "$rc" -ne 0 ] && { [ "$other" = yes ] || [ -z "$closed" ]; }; then
		die "restorecon failed (exit status $rc): not every file is labelled."
	fi
	if [ -n "$closed" ]; then
		echo "Every file is labelled, except in the FUSE mounts root cannot enter:"
		printf '%s\n' "$closed" | sed 's/^/  /'
	else
		echo "Every file is labelled."
	fi
	echo "What the policy would refuse is in the kernel log:"
	echo "  dmesg | grep avc:"
	;;
s6)
	need_root
	[ -d /sys/fs/selinux ] ||
		die "SELinux is not on in the running kernel: sh $0 status says what is missing."
	policy_loaded || die "No policy is loaded: sh $0 status says what is missing."
	command -v semanage >/dev/null && command -v restorecon >/dev/null ||
		die "semanage and restorecon are missing: sudo sh $0 packages"
	l=$(root_label)
	[ "${l#*:root_t}" != "$l" ] || die "The files are not labelled yet: sudo sh $0 relabel"
	s6_paths || die "/sbin/init is not s6-linux-init's init script, or there is no s6-supervise; this step is for s6-linux-init."
	case "$init$supervise" in
	*[!A-Za-z0-9/._-]*) die "$init or $supervise has a character a file context cannot take as it is." ;;
	esac
	case "$(sed -n 1p "$rcinit")" in
	'#!'*/sh | '#!'*'/sh '*) ;;
	*) die "$rcinit is not a sh script; this step cannot add its line to it." ;;
	esac
	s6_contexts | while read -r t k p; do
		has_context "$t" "$k" "$p" && continue
		semanage fcontext --add --seuser system_u --type "$t" --ftype "$k" "$p" </dev/null 2>/dev/null ||
			semanage fcontext --modify --seuser system_u --type "$t" --ftype "$k" "$p" </dev/null
		echo "file context: $p is $t"
	done
	install -m 755 "$here/rc.selinux" "$hook"
	echo "installed $hook"
	if ! grep -q " $mark\$" "$rcinit"; then
		# Second, after the line that names the shell. The line cannot
		# fail, with or without our script: rc.init stops at a failure.
		new=$rcinit.wmutils-new
		cp -p "$rcinit" "$new"
		{
			sed -n 1p "$rcinit"
			echo "[ ! -x $hook ] || $hook || : $mark"
			sed 1d "$rcinit"
		} >"$new"
		if ! sh -n "$new"; then
			rm -f "$new"
			die "$rcinit with our line is not a script sh takes; left as it was."
		fi
		mv "$new" "$rcinit"
		echo "$rcinit runs it first"
	fi
	restorecon -F "$init" "$supervise"
	restorecon -RF "$scripts"
	s6_labelled || die "$init, $supervise and $rcinit did not get their types; undo with: sudo sh $0 s6-undo"
	echo "$init is init_exec_t; $supervise and $scripts/* are initrc_exec_t"
	echo "Nothing changes until the next boot. SELinux stays permissive."
	echo "Next: a reboot. After it: sh $0 status, and"
	echo "  ps -eo label,args      which domain everything runs in"
	echo "  sudo dmesg | grep avc: what the policy would refuse"
	;;
s6-undo)
	need_root
	s6_paths || die "/sbin/init is not s6-linux-init's init script; there is nothing of the s6 step."
	s6_undo
	echo "After the next boot everything runs in the kernel's own domain again."
	;;
remove)
	need_root
	s6_undo
	if ours; then
		link=$(cat "$orig")
		for f in "$profile"/* "$profile"/.[!.]*; do
			[ -e "$f" ] || continue
			case "${f##*/}" in
			parent | make.defaults | wmutils.orig) ;;
			*) die "$profile has $f, which is not ours; left as it is." ;;
			esac
		done
		rm -f "$profile/parent" "$profile/make.defaults" "$orig"
		rmdir "$profile"
		ln -s "$link" "$profile"
		echo "$profile is a link to $link again"
	fi
	if [ -f "$cmdline" ] && lsm=$(lsm_list); then
		cur=$(cat "$cmdline")
		out=
		set -f
		for w in $cur; do
			case "$w" in
			"lsm=$lsm" | enforcing=0) ;;
			*) out="$out${out:+ }$w" ;;
			esac
		done
		set +f
		if [ "$out" != "$cur" ]; then
			printf '%s\n' "$out" >"$cmdline.new"
			mv "$cmdline.new" "$cmdline"
			echo "took lsm=$lsm and enforcing=0 out of $cmdline"
		fi
	fi
	for f in "$dracutconf" "$start"; do
		if [ -e "$f" ]; then
			rm -f "$f"
			echo "removed $f"
		fi
	done
	rmdir "${dracutconf%/*}" 2>/dev/null || true
	echo "The next initramfs and UKI (emerge --config sys-kernel/asahi-kernel) boot without SELinux."
	echo "The packages are still there: emerge --ask --update --deep --newuse @world builds them"
	echo "without SELinux again, and emerge --ask --depclean removes the tools and policies."
	echo "The labels stay on the files, where nothing reads them."
	;;
*)
	echo "usage: sh $0 [status|profile|packages|boot|relabel|s6|s6-undo|remove]" >&2
	exit 2
	;;
esac
