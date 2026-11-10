#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Put the sound system ahead of everything else on the CPU, and keep the
# speakers from popping when playback starts or stops: add the desktop user
# to the pipewire group, whose limits let PipeWire and WirePlumber run their
# audio threads in real time by themselves, without RTKit, and install the
# drop-ins: the priorities of PipeWire and pipewire-pulse, the speaker DSP
# kept running when playback stops, and zeroramp in front of it, from a
# copy of asahi-audio's graph that a Portage hook keeps up to date. Run as
# root:
#   sudo sh audio/install.sh                 set it up for the user running sudo
#   sudo sh audio/install.sh install USER    for another user
#   sudo sh audio/install.sh remove [USER]   undo it
# Then quit Window Maker and start X from a new login, or reboot: the limits
# are set at login. If the user was in the group already, restarting
# PipeWire is enough: setsid -f gentoo-pipewire-launcher restart
set -eu

here=$(cd "$(dirname "$0")" && pwd)
name=99-wmutils-audio.conf
# The project was utils-mydesktop until November 2026: what it installed
# under that name goes once the same is there under the new one.
oldname=99-utils-mydesktop-audio.conf
oldshare=/usr/local/share/utils-mydesktop/audio
# Where the drop-ins go under /etc; here they are in the last part.
dropins="pipewire/pipewire.conf.d pipewire/pipewire-pulse.conf.d wireplumber/wireplumber.conf.d"
# asahi-audio's speaker DSP graph, where its copy with zeroramp goes, and
# the Portage hook that writes the copy again when asahi-audio is updated.
graph=/usr/share/asahi-audio/j314/graph.json
share=/usr/local/share/wmutils/audio
hook=/etc/portage/env/media-libs/asahi-audio
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

# The hook file belongs to us only if it starts with our marker, or with
# the one of the old name.
ours() {
	[ -f "$hook" ] && head -n 1 "$hook" | grep -Eq '^# (wmutils|utils-mydesktop): '
}

# Removes what was installed under the old name.
forget_old() {
	for d in $dropins; do
		if [ -e "/etc/$d/$oldname" ]; then
			rm -f "/etc/$d/$oldname"
			echo "removed /etc/$d/$oldname (the old name)"
		fi
	done
	for f in "$oldshare/j314-graph.json" "$oldshare/zeroramp.py"; do
		if [ -e "$f" ]; then
			rm -f "$f"
			echo "removed $f (the old name)"
		fi
	done
	rmdir "$oldshare" "${oldshare%/*}" 2>/dev/null || true
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
	# The WirePlumber drop-in replaces asahi-audio's DSP rules with the
	# J314's, which would leave any other Mac without its DSP.
	if ! tr '\0' '\n' 2>/dev/null </proc/device-tree/compatible | grep -qx 'apple,j314[sc]'; then
		echo "This is not a MacBook Pro 14\" (J314), which the WirePlumber drop-in is for." >&2
		exit 1
	fi
	if [ ! -f "$graph" ]; then
		echo "There is no $graph (media-libs/asahi-audio)." >&2
		exit 1
	fi
	for tool in python3 spa-json-dump; do
		if ! command -v "$tool" >/dev/null; then
			echo "$tool is missing; zeroramp.py needs it." >&2
			exit 1
		fi
	done
	if [ -e "$hook" ] && ! ours; then
		echo "$hook exists and is not ours; add the contents of" >&2
		echo "$here/asahi-audio.bashrc to it by hand." >&2
		exit 1
	fi
	added=no
	if member; then
		echo "$user is in the $group group already"
	else
		gpasswd -a "$user" "$group" >/dev/null
		echo "added $user to the $group group"
		added=yes
	fi
	# The copy of the graph before the drop-in that names it.
	install -D -m 644 "$here/zeroramp.py" "$share/zeroramp.py"
	echo "installed $share/zeroramp.py"
	python3 -I "$share/zeroramp.py" "$graph" "$share/j314-graph.json"
	echo "wrote $share/j314-graph.json, the speaker DSP with zeroramp"
	install -D -m 644 "$here/asahi-audio.bashrc" "$hook"
	echo "installed $hook"
	for d in $dropins; do
		install -D -m 644 "$here/${d#*/}/$name" "/etc/$d/$name"
		echo "installed /etc/$d/$name"
	done
	forget_old
	if [ "$added" = yes ]; then
		echo "Quit Window Maker and start X from a new login (or reboot): PipeWire then runs in real time."
	else
		echo "Restart PipeWire to load the drop-ins, as $user: setsid -f gentoo-pipewire-launcher restart"
	fi
	;;
remove)
	forget_old
	for d in $dropins; do
		f=/etc/$d/$name
		if [ -e "$f" ]; then
			rm -f "$f"
			echo "removed $f"
		fi
		# The directories too, if nothing else is left in them.
		rmdir "/etc/$d" "/etc/${d%/*}" 2>/dev/null || true
	done
	if ours; then
		rm -f "$hook"
		echo "removed $hook"
		rmdir "${hook%/*}" 2>/dev/null || true
	fi
	for f in "$share/j314-graph.json" "$share/zeroramp.py"; do
		if [ -e "$f" ]; then
			rm -f "$f"
			echo "removed $f"
		fi
	done
	rmdir "$share" "${share%/*}" 2>/dev/null || true
	if member; then
		gpasswd -d "$user" "$group" >/dev/null
		echo "removed $user from the $group group"
	fi
	echo "After a new login (or reboot) PipeWire goes back to RTKit's priorities and asahi-audio's speaker DSP."
	;;
esac
