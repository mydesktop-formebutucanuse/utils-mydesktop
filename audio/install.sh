#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Put the sound system ahead of everything else on the CPU: add the desktop
# user to the pipewire group, whose limits let PipeWire and WirePlumber run
# their audio threads in real time by themselves, without RTKit, and install
# the drop-ins with the priorities of PipeWire and pipewire-pulse. Run as
# root:
#   sudo sh audio/install.sh                 set it up for the user running sudo
#   sudo sh audio/install.sh install USER    for another user
#   sudo sh audio/install.sh remove [USER]   undo it
# Then quit Window Maker and start X from a new login, or reboot: the limits
# are set at login.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
name=99-utils-mydesktop-audio.conf
group=pipewire
action=${1:-install}
user=${2:-${SUDO_USER:-}}

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 $*" >&2
	exit 1
fi

case "$action" in
install|remove) ;;
*)
	echo "usage: sh $0 [install|remove] [USER]" >&2
	exit 2
	;;
esac

if [ -z "$user" ] || [ "$user" = root ] || ! id -u "$user" >/dev/null 2>&1; then
	echo "Name the desktop user: sudo sh $0 $action USER" >&2
	exit 1
fi

member() {
	id -nG "$user" | tr ' ' '\n' | grep -qx "$group"
}

case "$action" in
install)
	# The limits themselves are PipeWire's: media-video/pipewire installs
	# /etc/security/limits.d/25-pw-rlimits.conf for the group.
	if ! getent group "$group" >/dev/null; then
		echo "There is no $group group (acct-group/pipewire)." >&2
		exit 1
	fi
	if ! grep -qs "^[[:space:]]*@$group[[:space:]].*rtprio" /etc/security/limits.d/*.conf; then
		echo "No rtprio limit for @$group in /etc/security/limits.d;" >&2
		echo "media-video/pipewire installs one." >&2
		exit 1
	fi
	if member; then
		echo "$user is in the $group group already"
	else
		gpasswd -a "$user" "$group" >/dev/null
		echo "added $user to the $group group"
	fi
	for conf in pipewire pipewire-pulse; do
		install -D -m 644 "$here/$conf.conf.d/$name" "/etc/pipewire/$conf.conf.d/$name"
		echo "installed /etc/pipewire/$conf.conf.d/$name"
	done
	echo "Quit Window Maker and start X from a new login (or reboot): PipeWire then runs in real time."
	;;
remove)
	for conf in pipewire pipewire-pulse; do
		f=/etc/pipewire/$conf.conf.d/$name
		if [ -e "$f" ]; then
			rm -f "$f"
			echo "removed $f"
		fi
		rmdir "${f%/*}" 2>/dev/null || true
	done
	rmdir /etc/pipewire 2>/dev/null || true
	if member; then
		gpasswd -d "$user" "$group" >/dev/null
		echo "removed $user from the $group group"
	fi
	echo "After a new login (or reboot) PipeWire goes back to RTKit's priorities."
	;;
esac
