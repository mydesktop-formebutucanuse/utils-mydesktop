# network

Keeps the machine from being found or reached on the networks it joins. A
firewall lets in nothing that arrives unasked and does not answer ping,
and NetworkManager gives a network neither the hostname nor the Wi-Fi
adapter's own MAC address. Connections the machine starts itself are not
limited.

| File | What it is |
|---|---|
| [install.sh](install.sh) | Installs the rest, loads the firewall and has NetworkManager read its configuration again |
| [iptables.rules](iptables.rules) | The IPv4 firewall |
| [ip6tables.rules](ip6tables.rules) | The IPv6 firewall |
| [firewall.start](firewall.start) | Loads both at every boot, from `/etc/local.d` |
| [conf.d/99-wmutils-network.conf](conf.d/99-wmutils-network.conf) | NetworkManager's defaults: no hostname in DHCP, temporary IPv6 addresses, a made-up MAC address for each Wi-Fi network |

Written for iptables 1.8.13 (legacy) and NetworkManager 1.58 with its own
DHCP client, on Gentoo with s6-rc.

## How it was

No firewall at all: the kernel's iptables modules were not even loaded.
Nothing listened for connections either (no sshd, Avahi, Samba or CUPS),
so there was no open port to find. But the machine answered ping, said
`Gentoo` as its hostname in every DHCP request, which a router or a phone's
hotspot shows in its list of devices, and used the Wi-Fi adapter's own MAC
address, which is the same on every network.

## The firewall

Both rules files fill one chain, `wmutils-in`, and `INPUT` jumps to
it first. It accepts:

- everything on the loopback interface;
- answers to what the machine sent, and the ICMP errors about it
  (conntrack's `ESTABLISHED` and `RELATED`);
- the DHCP and DHCPv6 servers' answers;
- for IPv6, router advertisements, neighbour solicitations and
  advertisements, and the router's multicast listener queries. IPv6 does
  not work without them: they are its ARP.

and drops the rest: connections to any port, ping, and whatever a scanner
sends. A dropped packet gets no answer at all, so to the sender a closed
port and no machine look the same.

The chain is our own so that nothing else is touched. `firewall.start`
loads the files with `iptables-restore --noflush`, which replaces that
chain in one step and leaves every other rule, and the policies of the
built-in chains, as they are. `install.sh remove` takes out exactly that
chain.

Forwarding is not filtered. It is off in the kernel
(`net.ipv4.ip_forward = 0`), so nothing passes through this machine.

To let something in, say sshd, add a line above the `DROP` in both files
and run `install.sh` again:

```
-A wmutils-in -p tcp -m tcp --dport 22 -j ACCEPT
```

## At boot

s6-rc's `init-local` runs the executable `/etc/local.d/*.start` scripts,
as OpenRC's `local` service does, and `install.sh` puts `firewall.start`
there as `wmutils-firewall.start`. `init-local` and NetworkManager
do not wait for each other, so the Wi-Fi may connect a moment before the
firewall loads. With nothing listening, that moment exposes only ping.

## What the network is told

The drop-in in `/etc/NetworkManager/conf.d` sets defaults for every
connection profile that does not set the property itself:

- `ipv4.dhcp-send-hostname` and `ipv6.dhcp-send-hostname` 0: no hostname
  in DHCP requests, so the router lists the machine without a name.
- `ipv6.ip6-privacy` 2: outgoing connections use temporary IPv6 addresses,
  which change every day.
- `wifi.cloned-mac-address` `stable`: a made-up MAC address for each Wi-Fi
  network, the same one every time on that network, so two networks cannot
  tell that the same machine was on both. A network that lets in only the
  MAC addresses it knows needs the real one: take the line out, or set
  `wifi.cloned-mac-address permanent` on that profile.

`connection.mdns` and `connection.llmnr` are not set: only systemd-resolved
answers those, and it does not run here. `wifi.scan-rand-mac-address` is
not set either: scans use a random MAC address by default.

## What stays visible

A machine on a network cannot hide that it is there. It answers ARP and
IPv6 neighbour solicitations, or nothing could send to it, and iptables
does not filter ARP. Whoever scans the same network still finds an IP
address with the made-up MAC address. They get no name, no answer to ping
and no port.

## Setting it up

```sh
sudo sh network/install.sh
```

The firewall is on at once. The NetworkManager settings take effect when a
network connects, so connect again:

```sh
nmcli connection up NAME
```

To check the firewall, which shows the chain's rules and, as the first
rule of `INPUT`, the jump to it:

```sh
sudo iptables -S | grep wmutils-in
```

```sh
sudo ip6tables -S | grep wmutils-in
```

To check the MAC address: `link/ether` is the made-up address and
`permaddr` the adapter's own.

```sh
ip link show wld0
```

The hostname is gone from the router's or hotspot's list of devices once
it has given out a new lease.

`sudo sh network/install.sh remove` undoes it: it takes the chain out of
iptables and ip6tables and removes the rules files, the boot script and
the NetworkManager drop-in.
