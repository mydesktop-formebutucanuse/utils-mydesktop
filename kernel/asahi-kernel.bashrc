# wmutils: kernel patches and a config check for sys-kernel/asahi-kernel.
# Installed as /etc/portage/env/sys-kernel/asahi-kernel by kernel/install.sh.
#
# Unlike /etc/portage/patches, a patch that no longer fits a new kernel
# does not stop the build: it is skipped with a warning, the others are
# still tried, and the kernel builds without it until it is updated.
#
# Once the config is final, the build stops if it leaves out an option in
# required.txt: better than an hour of compiling a kernel that would come up
# without its display or keyboard driver.

wmutils_patches() {
	local patch name
	pushd "${S}" >/dev/null || return 0
	for patch in /usr/local/share/wmutils/*.patch; do
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

wmutils_required() {
	local list=/usr/local/share/wmutils/required.txt
	local cfg=${WORKDIR}/modprep/.config
	[[ -f ${list} ]] || return 0
	if [[ ! -f ${cfg} ]]; then
		ewarn "wmutils: no ${cfg}, so the config was not checked."
		return 0
	fi
	# Every option the source defines, to tell one that is off from one
	# that a new kernel renamed or dropped.
	local known
	known=$(grep -rhoE --include='Kconfig*' \
		'^[[:space:]]*(menu)?config[[:space:]]+[A-Za-z0-9_]+' "${S}" |
		awk '{ print "CONFIG_" $2 }' | sort -u)
	local sym mods x off=() gone=()
	while read -r sym mods; do
		[[ ${sym} == CONFIG_* ]] || continue
		grep -qE "^${sym}=[ym]\$" "${cfg}" && continue
		if grep -qxF "${sym}" <<<"${known}"; then
			off+=( "${sym} (${mods})" )
		else
			gone+=( "${sym} (${mods})" )
		fi
	done <"${list}"
	for x in "${gone[@]}"; do
		ewarn "wmutils: ${PF} has no ${x}; check that the driver still builds."
	done
	if [[ ${#off[@]} -gt 0 ]]; then
		eerror "wmutils: the kernel config leaves out what this Mac needs:"
		for x in "${off[@]}"; do
			eerror "  ${x}"
		done
		eerror "See /etc/kernel/config.d/wmutils-trim.config and ${list}."
		if [[ ${WMUTILS_UNCHECKED} ]]; then
			ewarn "WMUTILS_UNCHECKED is set: building anyway."
		else
			die "kernel config lacks options this Mac needs (WMUTILS_UNCHECKED=1 builds anyway)"
		fi
	elif [[ ${#gone[@]} -eq 0 ]]; then
		einfo "wmutils: the config has everything in ${list##*/}."
	fi
}

post_src_prepare() {
	wmutils_patches
}

post_src_configure() {
	wmutils_required
}
