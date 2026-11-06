#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Set up Gentoo so that every sys-kernel/asahi-kernel build applies the
# panel backlight patches in this directory, each one if it fits, and only
# warns about one that does not. Run as root:
#   sudo sh kernel/install.sh          set it up (again after the patches change)
#   sudo sh kernel/install.sh remove   undo it
# The next kernel build (an update, or `emerge --oneshot sys-kernel/asahi-kernel`)
# picks it up.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
share=/usr/local/share/utils-mydesktop
hook=/etc/portage/env/sys-kernel/asahi-kernel
plain=/etc/portage/patches/sys-kernel/asahi-kernel

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 ${1:-}" >&2
	exit 1
fi

# The hook file belongs to us only if it starts with our marker.
ours() {
	[ -f "$hook" ] && head -n 1 "$hook" | grep -q '^# utils-mydesktop: '
}

case "${1:-install}" in
install)
	if [ -e "$hook" ] && ! ours; then
		echo "$hook exists and is not ours; add the contents of" >&2
		echo "$here/asahi-kernel.bashrc to it by hand." >&2
		exit 1
	fi
	mkdir -p "$share"
	# Patches that left the series go too.
	for p in "$share"/*.patch; do
		[ -e "$p" ] && [ ! -e "$here/${p##*/}" ] || continue
		rm -f "$p"
		echo "removed $p"
	done
	for p in "$here"/*.patch; do
		name=${p##*/}
		install -m 644 "$p" "$share/$name"
		echo "installed $share/$name"
		# The hook replaces a plain user patch, which would stop the
		# build instead of warning when it no longer fits.
		if [ -e "$plain/$name" ]; then
			rm -f "$plain/$name"
			echo "removed $plain/$name (the hook applies it now)"
		fi
	done
	rmdir "$plain" "$(dirname "$plain")" 2>/dev/null || true
	install -D -m 644 "$here/asahi-kernel.bashrc" "$hook"
	echo "installed $hook"
	echo "The next asahi-kernel build applies the patches, or warns about any that no longer fit."
	;;
remove)
	if ours; then
		rm -f "$hook"
		rmdir "$(dirname "$hook")" 2>/dev/null || true
		echo "removed $hook"
	fi
	rm -f "$share"/*.patch
	rmdir "$share" 2>/dev/null || true
	echo "removed the patches from $share"
	echo "The next asahi-kernel build is unpatched."
	;;
*)
	echo "usage: sh $0 [install|remove]" >&2
	exit 2
	;;
esac
