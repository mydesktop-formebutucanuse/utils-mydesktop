# Copyright 2026 Gentoo Authors
# Distributed under the terms of the GNU General Public License v2

EAPI=8

RUST_MIN_VER="1.85.0"
inherit cargo git-r3 udev

DESCRIPTION="Window Maker desktop utilities: mediakeyd, autobrightd, scd, backlight and logincookies"
HOMEPAGE="https://github.com/mydesktop-formebutucanuse/utils-mydesktop"
EGIT_REPO_URI="https://github.com/mydesktop-formebutucanuse/utils-mydesktop.git"

LICENSE="GPL-3+"
SLOT="0"

DEPEND="
	>=dev-db/sqlite-3.35:3
	x11-libs/libX11
	x11-libs/libXi
"
# backlight: dbus-send for the keyboard, pkill to signal autobrightd
RDEPEND="${DEPEND}
	sys-apps/dbus
	sys-process/procps
"

src_unpack() {
	git-r3_src_unpack
	cargo_live_src_unpack
}

src_install() {
	dobin "$(cargo_target_dir)"/{autobrightd,logincookies,mediakeyd,scd}
	dobin backlight/backlight
	# Lets the video group write the backlights, so autobrightd runs as
	# the desktop user.
	udev_dorules autobrightd/90-autobrightd.rules

	dodoc README.md
	local tool
	for tool in autobrightd backlight kernel logincookies mediakeyd scd; do
		docinto "${tool}"
		dodoc "${tool}"/README.md
	done
	docinto mediakeyd
	dodoc mediakeyd/config.example
	docinto logincookies
	dodoc logincookies/rules.example
}

pkg_postinst() {
	udev_reload
	elog "Start the daemons with the X session, e.g. in"
	elog "~/GNUstep/Library/WindowMaker/autostart (scd before the others):"
	elog "  scd &  mediakeyd &  autobrightd &"
	elog "autobrightd needs the desktop user in the video group; the udev rule"
	elog "takes effect for the backlights at the next boot, or after"
	elog "  udevadm trigger --action=add /sys/class/backlight/* /sys/class/leds/kbd_backlight"
	elog "Start LibreWolf with logincookies to keep its login cookies (see"
	elog "/usr/share/doc/${PF}/logincookies/README.md*)."
	elog "The kernel patches are not part of this package: see"
	elog "/usr/share/doc/${PF}/kernel/README.md*."
}

pkg_postrm() {
	udev_reload
}
