#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Install utils-mydesktop with emerge: puts the live ebuild
# x11-misc/utils-mydesktop-9999 into a local overlay, accepts its (empty)
# keywords, emerges it, and removes the udev rule a hand install left in
# /etc. Run as root, again whenever the ebuild changes:
#   sudo sh gentoo/install.sh [OVERLAY]   default overlay: mydesktop
# Code updates need no rerun: the ebuild builds the latest commit on GitHub,
#   sudo emerge --oneshot x11-misc/utils-mydesktop
set -eu

here=$(cd "$(dirname "$0")" && pwd)
pkg=x11-misc/utils-mydesktop
overlay=${1:-mydesktop}
keywords=/etc/portage/package.accept_keywords/$overlay
rule=90-autobrightd.rules

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 $*" >&2
	exit 1
fi

repo=$(portageq get_repo_path / "$overlay" 2>/dev/null) || repo=
if [ -z "$repo" ] || [ ! -d "$repo" ]; then
	echo "No overlay named $overlay in /etc/portage/repos.conf." >&2
	exit 1
fi

install -D -m 644 "$here/$pkg/utils-mydesktop-9999.ebuild" "$repo/$pkg/utils-mydesktop-9999.ebuild"
echo "installed $repo/$pkg/utils-mydesktop-9999.ebuild"

# A live ebuild has no keywords.
[ -f /etc/portage/package.accept_keywords ] && keywords=/etc/portage/package.accept_keywords
if ! grep -qs "^$pkg[[:space:]]" "$keywords"; then
	mkdir -p "$(dirname "$keywords")"
	echo "$pkg **" >>"$keywords"
	echo "added '$pkg **' to $keywords"
fi

emerge "$pkg"

# The package's rule is in /usr/lib/udev/rules.d; one with the same name in
# /etc would hide it.
packaged=$(pkg-config --variable=udevdir udev 2>/dev/null || echo /usr/lib/udev)/rules.d/$rule
if [ -e "/etc/udev/rules.d/$rule" ] && [ -e "$packaged" ]; then
	if cmp -s "/etc/udev/rules.d/$rule" "$packaged"; then
		rm -f "/etc/udev/rules.d/$rule"
		echo "removed /etc/udev/rules.d/$rule (the package installs $packaged)"
	else
		echo "/etc/udev/rules.d/$rule differs from $packaged and hides it; left in place."
	fi
fi
