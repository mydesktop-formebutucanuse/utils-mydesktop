# utils-mydesktop: panel backlight patches for sys-kernel/asahi-kernel.
# Installed as /etc/portage/env/sys-kernel/asahi-kernel by kernel/install.sh.
#
# Unlike /etc/portage/patches, a patch that no longer fits a new kernel
# does not stop the build: it is skipped with a warning, the others are
# still tried, and the kernel builds without it until it is updated.

utils_mydesktop_backlight_patches() {
	local patch name
	pushd "${S}" >/dev/null || return 0
	for patch in /usr/local/share/utils-mydesktop/*.patch; do
		[[ -f ${patch} ]] || continue
		name=${patch##*/}
		if patch -p1 -R -s -f --dry-run <"${patch}" >/dev/null 2>&1; then
			einfo "${name}: already in the source"
		elif patch -p1 -s -f --dry-run <"${patch}" >/dev/null 2>&1; then
			patch -p1 -s -f <"${patch}" || die "${name} failed to apply"
			einfo "${name}: applied"
		else
			ewarn "${name} does not fit ${PF}; building without it."
		fi
	done
	popd >/dev/null || die
}

post_src_prepare() {
	utils_mydesktop_backlight_patches
}
