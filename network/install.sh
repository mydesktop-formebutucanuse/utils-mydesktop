#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Keep this machine from being found or reached on the networks it joins:
# install and load a firewall that lets in nothing that arrives unasked and
# does not answer ping, have it loaded at every boot, and set
# NetworkManager's defaults so that the machine gives a network neither its
# hostname nor the Wi-Fi adapter's own MAC address. Run as root:
#   sudo sh network/install.sh          set it up (again after this directory changes)
#   sudo sh network/install.sh remove   undo it
# The firewall is on at once. The NetworkManager settings take effect the
# next time a network connects: nmcli connection up NAME
set -eu

here=$(cd "$(dirname "$0")" && pwd)
share=/usr/local/share/wmutils/network
start=/etc/local.d/wmutils-firewall.start
nmconf=/etc/NetworkManager/conf.d/99-wmutils-network.conf
# The chain the rules files fill and firewall.start has INPUT jump to.
chain=wmutils-in
# The project was utils-mydesktop until November 2026: what it installed
# under that name goes once the same is there under the new one.
oldshare=/usr/local/share/utils-mydesktop/network
oldstart=/etc/local.d/utils-mydesktop-firewall.start
oldnmconf=/etc/NetworkManager/conf.d/99-utils-mydesktop-network.conf
oldchain=utils-mydesktop-in

if [ "$(id -u)" -ne 0 ]; then
	echo "Run as root: sudo sh $0 ${1:-}" >&2
	exit 1
fi

# NetworkManager reads its configuration again; profiles use it from their
# next connect.
reload() {
	if command -v nmcli >/dev/null && nmcli general reload conf 2>/dev/null; then
		echo "NetworkManager has read its configuration again"
	else
		echo "NetworkManager is not running; it reads the configuration when it starts"
	fi
}

# Takes the chain $1 out of INPUT and deletes it, for IPv4 and IPv6.
drop_chain() {
	for ipt in iptables ip6tables; do
		command -v "$ipt" >/dev/null || continue
		# -S fails if there is no such chain.
		"$ipt" -w -S "$1" >/dev/null 2>&1 || continue
		while "$ipt" -w -D INPUT -j "$1" 2>/dev/null; do :; done
		"$ipt" -w -F "$1"
		"$ipt" -w -X "$1"
		echo "removed the $1 chain from $ipt"
	done
}

# Removes what was installed under the old name.
forget_old() {
	drop_chain "$oldchain"
	for f in "$oldstart" "$oldshare/iptables.rules" "$oldshare/ip6tables.rules" "$oldnmconf"; do
		if [ -e "$f" ]; then
			rm -f "$f"
			echo "removed $f (the old name)"
		fi
	done
	rmdir "$oldshare" "${oldshare%/*}" 2>/dev/null || true
}

case "${1:-install}" in
install)
	for tool in iptables iptables-restore ip6tables ip6tables-restore; do
		if ! command -v "$tool" >/dev/null; then
			echo "$tool is missing (net-firewall/iptables)." >&2
			exit 1
		fi
	done
	# Without it nothing would load the firewall at boot.
	if [ ! -d "${start%/*}" ]; then
		echo "There is no ${start%/*}, whose *.start scripts s6-rc's init-local" >&2
		echo "or OpenRC's local service runs at boot." >&2
		exit 1
	fi
	if [ ! -d /etc/NetworkManager ]; then
		echo "There is no /etc/NetworkManager (net-misc/networkmanager)." >&2
		exit 1
	fi
	# The rules before the script that loads them.
	for f in iptables.rules ip6tables.rules; do
		install -D -m 644 "$here/$f" "$share/$f"
		echo "installed $share/$f"
	done
	install -D -m 755 "$here/firewall.start" "$start"
	echo "installed $start"
	"$start"
	echo "loaded the firewall: INPUT jumps to $chain, for IPv4 and IPv6"
	install -D -m 644 "$here/conf.d/${nmconf##*/}" "$nmconf"
	echo "installed $nmconf"
	# Only now, with the new chain loaded: the firewall is never off.
	forget_old
	reload
	echo "Connect again for the new MAC address and a DHCP lease without the hostname:"
	echo "  nmcli connection up NAME   (nmcli connection show --active lists the names)"
	;;
remove)
	drop_chain "$chain"
	forget_old
	for f in "$start" "$share/iptables.rules" "$share/ip6tables.rules" "$nmconf"; do
		if [ -e "$f" ]; then
			rm -f "$f"
			echo "removed $f"
		fi
	done
	# The directories too, if nothing else is left in them.
	rmdir "$share" "${share%/*}" "${nmconf%/*}" 2>/dev/null || true
	reload
	echo "Nothing filters incoming connections now. After the next connect the network"
	echo "gets the hostname and the Wi-Fi adapter's own MAC address again."
	;;
*)
	echo "usage: sh $0 [install|remove]" >&2
	exit 2
	;;
esac
