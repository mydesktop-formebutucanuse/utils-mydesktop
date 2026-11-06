#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Set up Gentoo so that every sys-kernel/asahi-kernel build applies the
# 1/16-nit backlight patch when it fits and only warns when it does not.
# Run as root:
#   sudo sh kernel/install.sh          set it up
#   sudo sh kernel/install.sh remove   undo it
# The next kernel build (an update, or `emerge --oneshot sys-kernel/asahi-kernel`)
# picks it up.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
name=0001-drm-apple-backlight-1-16-nit.patch
share=/usr/local/share/utils-mydesktop
hook=/etc/portage/env/sys-kernel/asahi-kernel
plain=/etc/portage/patches/sys-kernel/asahi-kernel/$name
marker="# utils-mydesktop: 1/16-nit panel backlight"

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 ${1:-}" >&2
	exit 1
fi

# The hook file belongs to us only if it starts with our marker line.
ours() {
	[ -f "$hook" ] && [ "$(head -n 1 "$hook")" = "$marker for sys-kernel/asahi-kernel." ]
}

case "${1:-install}" in
install)
	if [ -e "$hook" ] && ! ours; then
		echo "$hook exists and is not ours; add the contents of" >&2
		echo "$here/asahi-kernel.bashrc to it by hand." >&2
		exit 1
	fi
	install -D -m 644 "$here/$name" "$share/$name"
	install -D -m 644 "$here/asahi-kernel.bashrc" "$hook"
	# The hook replaces the plain user patch, which would stop the build
	# instead of warning when it no longer fits.
	if [ -e "$plain" ]; then
		rm -f "$plain"
		rmdir -p "$(dirname "$plain")" 2>/dev/null || true
		echo "removed $plain (the hook applies it now)"
	fi
	echo "installed $share/$name"
	echo "installed $hook"
	echo "The next asahi-kernel build applies the patch, or warns if it no longer fits."
	;;
remove)
	if ours; then
		rm -f "$hook"
		rmdir -p "$(dirname "$hook")" 2>/dev/null || true
		echo "removed $hook"
	fi
	rm -f "$share/$name"
	rmdir "$share" 2>/dev/null || true
	echo "removed $share/$name"
	echo "The next asahi-kernel build is unpatched (whole nits)."
	;;
*)
	echo "usage: sh $0 [install|remove]" >&2
	exit 2
	;;
esac
