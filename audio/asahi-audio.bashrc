# wmutils: zeroramp in front of asahi-audio's speaker DSP.
# Installed as /etc/portage/env/media-libs/asahi-audio by audio/install.sh.
#
# WirePlumber runs the speaker DSP from a copy of asahi-audio's
# j314/graph.json with zeroramp in front (see audio/README.md). Each time
# asahi-audio is installed or updated, this writes the copy again from the
# new graph, so that the copy never names filter files the update removed.
# If zeroramp cannot be added, the copy is asahi-audio's graph as it is.

wmutils_zeroramp() {
	local share=${EROOT%/}/usr/local/share/wmutils/audio
	local graph=${EROOT%/}/usr/share/asahi-audio/j314/graph.json
	[[ -f ${share}/zeroramp.py ]] || return 0
	if python3 -I "${share}/zeroramp.py" "${graph}" "${share}/j314-graph.json"; then
		einfo "wmutils: wrote ${share}/j314-graph.json, the new speaker DSP with zeroramp"
	elif cp "${graph}" "${share}/j314-graph.json"; then
		ewarn "wmutils: zeroramp could not be added to the new speaker DSP;"
		ewarn "${share}/j314-graph.json is asahi-audio's graph as it is."
	else
		eerror "wmutils: there is no ${graph} to copy; the speaker DSP"
		eerror "still runs from the old copy in ${share}. See audio/README.md."
		return 0
	fi
	einfo "Restart PipeWire as the desktop user to use it: setsid -f gentoo-pipewire-launcher restart"
}

post_pkg_postinst() {
	wmutils_zeroramp
}
