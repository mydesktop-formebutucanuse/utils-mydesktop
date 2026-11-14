# Copyright 2026 Gentoo Authors
# Distributed under the terms of the GNU General Public License v2

EAPI=8

RUST_MIN_VER="1.85.0"
inherit cargo git-r3 toolchain-funcs udev

DESCRIPTION="Window Maker desktop utilities: mediakeyd, autobrightd, scd, ledclock, nsrun, dock apps"
HOMEPAGE="https://github.com/mydesktop-formebutucanuse/wmutils"
EGIT_REPO_URI="https://github.com/mydesktop-formebutucanuse/wmutils.git"

# GPL-2+: the dock apps wmclock and wmacpi (dockapps/NOTICE)
LICENSE="GPL-3+ GPL-2+"
SLOT="0"

# libdockapp, libXext, libXpm and libXrender: the dock apps
# libXft: ledclock
DEPEND="
	>=x11-libs/libdockapp-0.7:=
	x11-libs/libX11
	x11-libs/libXext
	x11-libs/libXft
	x11-libs/libXi
	x11-libs/libXpm
	x11-libs/libXrender
"
# backlight: dbus-send for the keyboard, pkill to signal autobrightd
# nsrun: bwrap for the namespaces, xauth and xrdb for the X cookies and
# resources
# The package was x11-misc/utils-mydesktop until November 2026 and installed
# the same files: emerge takes it off once this one is merged. So it does
# with Gentoo's wmclock and wmacpi, which this package's dock apps replace.
RDEPEND="${DEPEND}
	!x11-misc/utils-mydesktop
	!x11-plugins/wmacpi
	!x11-plugins/wmclock
	sys-apps/bubblewrap
	sys-apps/dbus
	sys-process/procps
	x11-apps/xauth
	x11-apps/xrdb
"
BDEPEND="virtual/pkgconfig"

src_unpack() {
	git-r3_src_unpack
	# cargo.eclass no longer changes into ${S} itself (gentoo commit 0ed513f46179)
	pushd "${S}" >/dev/null || die
	cargo_live_src_unpack
	popd >/dev/null || die
}

src_compile() {
	cargo_src_compile
	emake -C dockapps CC="$(tc-getCC)" PKG_CONFIG="$(tc-getPKG_CONFIG)" \
		PREFIX="${EPREFIX}/usr"
}

src_install() {
	dobin "$(cargo_target_dir)"/{autobrightd,ledclock,mediakeyd,scd}
	# wmclock, wmacpi and wmacpi-cli, their man pages, and wmclock's day
	# and month names in /usr/share/wmclock
	emake -C dockapps DESTDIR="${D}" PREFIX="${EPREFIX}/usr" install
	dobin backlight/backlight
	dobin nsrun/nsrun nsrun/nsrun-xserver
	# Lets the video group write the backlights, so autobrightd runs as
	# the desktop user.
	udev_dorules autobrightd/90-autobrightd.rules

	dodoc README.md
	local tool
	for tool in autobrightd backlight dockapps kernel ledclock mediakeyd nsrun scd; do
		docinto "${tool}"
		dodoc "${tool}"/README.md
	done
	docinto dockapps
	dodoc dockapps/NOTICE
	docinto mediakeyd
	dodoc mediakeyd/config.example
	docinto nsrun
	dodoc nsrun/xnamespaces.example
}

pkg_postinst() {
	udev_reload
	elog "Start the daemons with the X session, e.g. in"
	elog "~/GNUstep/Library/WindowMaker/autostart (scd before the others):"
	elog "  scd &  mediakeyd &  autobrightd &"
	elog "autobrightd needs the desktop user in the video group; the udev rule"
	elog "takes effect for the backlights at the next boot, or after"
	elog "  udevadm trigger --action=add /sys/class/backlight/* /sys/class/leds/kbd_backlight"
	elog "The dock apps are wmclock and wmacpi; start the battery one as"
	elog "  wmacpi -f"
	elog "and drag both into the Dock: /usr/share/doc/${PF}/dockapps/README.md*."
	elog "ledclock is a clock on the desktop: start it with the session"
	elog "  ledclock &"
	elog "and drag it to its place, or a corner of it to its size."
	elog "nsrun -x needs the X server started through nsrun-xserver: see"
	elog "/usr/share/doc/${PF}/nsrun/README.md*."
	elog "The kernel patches are not part of this package: see"
	elog "/usr/share/doc/${PF}/kernel/README.md*."
}

pkg_postrm() {
	udev_reload
}
