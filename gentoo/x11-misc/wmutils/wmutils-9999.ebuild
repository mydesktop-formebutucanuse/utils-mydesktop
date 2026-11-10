# Copyright 2026 Gentoo Authors
# Distributed under the terms of the GNU General Public License v2

EAPI=8

RUST_MIN_VER="1.85.0"
inherit cargo git-r3 udev

DESCRIPTION="Window Maker desktop utilities: mediakeyd, autobrightd, scd, notchbar, backlight and nsrun"
HOMEPAGE="https://github.com/mydesktop-formebutucanuse/wmutils"
EGIT_REPO_URI="https://github.com/mydesktop-formebutucanuse/wmutils.git"

LICENSE="GPL-3+"
SLOT="0"

DEPEND="
	x11-libs/libX11
	x11-libs/libXft
	x11-libs/libXi
"
# backlight: dbus-send for the keyboard, pkill to signal autobrightd
# nsrun: bwrap for the namespaces, xauth and xrdb for the X cookies and
# resources
# The package was x11-misc/utils-mydesktop until November 2026 and installed
# the same files: emerge takes it off once this one is merged.
RDEPEND="${DEPEND}
	!x11-misc/utils-mydesktop
	sys-apps/bubblewrap
	sys-apps/dbus
	sys-process/procps
	x11-apps/xauth
	x11-apps/xrdb
"

src_unpack() {
	git-r3_src_unpack
	# cargo.eclass no longer changes into ${S} itself (gentoo commit 0ed513f46179)
	pushd "${S}" >/dev/null || die
	cargo_live_src_unpack
	popd >/dev/null || die
}

src_install() {
	dobin "$(cargo_target_dir)"/{autobrightd,mediakeyd,notchbar,scd}
	dobin backlight/backlight
	dobin nsrun/nsrun nsrun/nsrun-xserver
	# Lets the video group write the backlights, so autobrightd runs as
	# the desktop user.
	udev_dorules autobrightd/90-autobrightd.rules

	dodoc README.md
	local tool
	for tool in autobrightd backlight kernel mediakeyd notchbar nsrun scd; do
		docinto "${tool}"
		dodoc "${tool}"/README.md
	done
	docinto mediakeyd
	dodoc mediakeyd/config.example
	docinto nsrun
	dodoc nsrun/xnamespaces.example
}

pkg_postinst() {
	udev_reload
	elog "Start the daemons with the X session, e.g. in"
	elog "~/GNUstep/Library/WindowMaker/autostart (scd before the others):"
	elog "  scd &  mediakeyd &  autobrightd &  notchbar &"
	elog "notchbar needs the notch shown: appledrm.show_notch=1 (see its README)."
	elog "autobrightd needs the desktop user in the video group; the udev rule"
	elog "takes effect for the backlights at the next boot, or after"
	elog "  udevadm trigger --action=add /sys/class/backlight/* /sys/class/leds/kbd_backlight"
	elog "nsrun -x needs the X server started through nsrun-xserver: see"
	elog "/usr/share/doc/${PF}/nsrun/README.md*."
	elog "The kernel patches are not part of this package: see"
	elog "/usr/share/doc/${PF}/kernel/README.md*."
}

pkg_postrm() {
	udev_reload
}
