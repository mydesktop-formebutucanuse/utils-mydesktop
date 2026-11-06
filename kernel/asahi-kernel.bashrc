# utils-mydesktop: 1/16-nit panel backlight for sys-kernel/asahi-kernel.
# Installed as /etc/portage/env/sys-kernel/asahi-kernel by kernel/install.sh.
#
# Unlike /etc/portage/patches, a patch that no longer fits a new kernel
# does not stop the build: it is skipped with a warning, and the backlight
# counts whole nits again until the patch is updated.

utils_mydesktop_backlight_patch() {
	local patch=/usr/local/share/utils-mydesktop/0001-drm-apple-backlight-1-16-nit.patch
	local name=${patch##*/}
	[[ -f ${patch} ]] || return 0
	pushd "${S}" >/dev/null || return 0
	if patch -p1 -R -s -f --dry-run <"${patch}" >/dev/null 2>&1; then
		einfo "${name}: already in the source"
	elif patch -p1 -s -f --dry-run <"${patch}" >/dev/null 2>&1; then
		patch -p1 -s -f <"${patch}" || die "${name} failed to apply"
		einfo "${name}: applied (panel backlight in 1/16 nit)"
	else
		ewarn "${name} does not fit ${PF}; building without it."
		ewarn "The panel backlight counts whole nits until the patch is updated."
	fi
	popd >/dev/null || die
}

post_src_prepare() {
	utils_mydesktop_backlight_patch
}
