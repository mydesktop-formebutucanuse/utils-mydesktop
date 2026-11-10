#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Install wmutils with emerge: puts the live ebuild
# x11-misc/wmutils-9999 into a local overlay, accepts its (empty)
# keywords and those of libdockapp, which the dock apps need and Gentoo has
# not keyworded for arm64, emerges it, and removes the udev rule a hand
# install left in /etc. The package was x11-misc/utils-mydesktop until
# November 2026: its ebuild and keywords go, and it leaves the world file, so
# that emerge takes it off once the new one, which blocks it, is merged. So
# do Gentoo's x11-plugins/wmclock and x11-plugins/wmacpi, which the dock apps
# of this package replace. Run as root, again whenever the ebuild changes:
#   sudo sh gentoo/install.sh [OVERLAY]   default overlay: mydesktop
# Code updates need no rerun: the ebuild builds the latest commit on GitHub,
#   sudo emerge --oneshot x11-misc/wmutils
set -eu

here=$(cd "$(dirname "$0")" && pwd)
pkg=x11-misc/wmutils
oldpkg=x11-misc/utils-mydesktop
# Packages that install the files of the dock apps.
replaced="x11-plugins/wmclock x11-plugins/wmacpi"
# Wanted by the dock apps; in Gentoo without an arm64 keyword.
dockapplib=x11-libs/libdockapp
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

install -D -m 644 "$here/$pkg/wmutils-9999.ebuild" "$repo/$pkg/wmutils-9999.ebuild"
echo "installed $repo/$pkg/wmutils-9999.ebuild"

# A live ebuild has no keywords.
[ -f /etc/portage/package.accept_keywords ] && keywords=/etc/portage/package.accept_keywords
if ! grep -qs "^$pkg[[:space:]]" "$keywords"; then
	mkdir -p "$(dirname "$keywords")"
	echo "$pkg **" >>"$keywords"
	echo "added '$pkg **' to $keywords"
fi
# Any file of package.accept_keywords may accept libdockapp already.
if ! grep -rqs "^$dockapplib[[:space:]]" /etc/portage/package.accept_keywords; then
	echo "$dockapplib **" >>"$keywords"
	echo "added '$dockapplib **' to $keywords"
fi

# What the old name left in the overlay and in the keywords.
if [ -e "$repo/$oldpkg/${oldpkg#*/}-9999.ebuild" ]; then
	rm -f "$repo/$oldpkg/${oldpkg#*/}-9999.ebuild"
	rmdir "$repo/$oldpkg" 2>/dev/null || true
	echo "removed $repo/$oldpkg/${oldpkg#*/}-9999.ebuild (the old name)"
fi
if grep -qs "^$oldpkg[[:space:]]" "$keywords"; then
	sed -i "\\|^$oldpkg[[:space:]]|d" "$keywords"
	echo "removed '$oldpkg **' from $keywords"
fi

# In the world file a blocked package would stop emerge: it only takes one
# off by itself when nothing asks for it.
for blocked in $oldpkg $replaced; do
	if portageq has_version / "$blocked"; then
		emerge --deselect "$blocked"
	fi
done

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
