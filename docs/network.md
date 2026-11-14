# Network settings

Core manages connections through NetworkManager on the system D-Bus, from the ordinary-user daemon through `crates/network`. NetworkManager is required, in the current Arch Linux release. There is no other backend: when it is stopped or absent, the pages and interfaces report the error and show no sample values. Reads use `NO_AUTO_START`, so inspecting state never starts a stopped service. The Proxy page uses GSettings instead; see [Proxy](#proxy).

An adapter is physical when NetworkManager does not flag it as software and `/sys/class/net/<name>/device` exists; VMware adapters look like Ethernet otherwise. The Wi-Fi P2P companion and loopback are not adapters.

Speeds and data used are derived from `/sys/class/net/<adapter>/statistics` of the adapter behind NetworkManager's primary connection, polled once a second only while the page is open. VPN or container traffic carried by that adapter counts toward it. NetworkManager has no per-application accounting or traffic limits, so there is no Data usage page.

Removed previews: Windows network profiles (Public or Private), manual VPN forms, VPN roaming and metered policies, hotspot sharing source and method and idle shutdown, network reset and advanced sharing. They have no NetworkManager counterpart; a VPN is created by importing its file, and a hotspot shares the primary connection.

## Network types

| Type | Behavior |
| --- | --- |
| Open, Enhanced Open (OWE) | Connects without a password. |
| WPA, WPA2, WPA2/WPA3 Personal | `wpa-psk` with an 8 to 63 character password or 64 hexadecimal digits. |
| WPA3 Personal | `sae` with the same password limits. |
| Enterprise (802.1X) | Listed. An existing profile can connect and keeps its preferences editable; a new one cannot be created here, because safe setup needs an EAP method, identity and CA verification. Missing credentials are reported; use `nm-connection-editor` or `nmcli --ask`. |
| WEP | Listed and marked insecure. An existing profile can connect; a new one cannot be created. |

## Connecting and secrets

A new network is added with `persist=memory`, and saved to disk only once it is activated, so a wrong password leaves no broken profile. Activation returns immediately; the result arrives through NetworkManager's signals and `Changed("network", state)`. A failure with reason `no-secrets` or `supplicant-disconnect` offers a password retry. A VPN or hotspot that fails to start reports its name.

Passwords are only written to NetworkManager. They are never read back, returned, logged, emitted in `Changed`, or stored in `network.toml`; this includes the hotspot password. Their parser errors do not echo the request. Settings has no secret agent: a profile whose password is agent-owned or asked for each time fails with missing secrets, and entering a password stores it in NetworkManager with `psk-flags=0`. The dialog states this. The CLI reads values on stdin with `-`, which keeps passwords out of the process list. Session D-Bus requests remain visible to processes of the same user.

Profile changes read the settings, change the fields Settings owns and send them back through `Update`; NetworkManager keeps stored secrets when none are sent (verified on NetworkManager 1.58).

## Addressing and DNS

IP assignment is Automatic (DHCP) or Manual with one IPv4 address, prefix length and optional gateway; further addresses set elsewhere are kept. IPv6 is automatic or off. Other methods set elsewhere, such as `shared` or `link-local`, are shown and kept until replaced. DNS server assignment is Automatic, optionally with added servers, or Manual with at least one IPv4 or IPv6 server, written as `ipv4.dns-data` and `ipv6.dns-data` with `ignore-auto-dns`. The legacy `addresses` and `dns` forms are replaced by the data forms. DNS over TLS (`connection.dns-over-tls`) appears only when NetworkManager's DNS mode is `systemd-resolved`, the only resolver that applies it.

A change to a connected profile is applied at once with `Device.Reapply`; if that fails, the profile keeps the change and the error asks to reconnect. Settings does not offer an automatic rollback: the page stays usable while offline and the previous values can be chosen again.

## Wi-Fi privacy and priority

Random hardware address writes `802-11-wireless.assigned-mac-address`: System default (unset), Off (`permanent`), On, fixed for this network (`stable`), On, new each connection (`random`). It takes effect on the next connection. Connection priority writes `connection.autoconnect-priority` as Low (-10), Normal (0) or High (10); other values set elsewhere are shown and kept.

Both are hidden when NetworkManager uses the `iwd` Wi-Fi backend (`wifi.backend=iwd` in its configuration files): with the default `wifi.iwd.autoconnect=true`, the manual states that `iwd` chooses networks itself and ignores `autoconnect-priority`, and its device code does not apply per-profile addresses. `iwd` also drops WEP networks. The manual marks this backend experimental; it has not been run with Settings.

## Ethernet

Connect asks NetworkManager to activate its best profile for the adapter (`ActivateConnection` with no profile); Disconnect uses `Device.Disconnect`, which also stops autoconnection until the next connect. An unplugged cable disables Connect. The profile shown is the active one, else the saved wired profile bound to the adapter, else one not bound to any adapter.

## VPN

Import accepts a WireGuard `.conf` or OpenVPN `.ovpn` file of at most 1 MiB by absolute path and runs `nmcli connection import type <wireguard|openvpn> file <path>`, which carries NetworkManager's own WireGuard parser and installed VPN-plugin importers. OpenVPN needs `networkmanager-openvpn`. NetworkManager autoconnects an imported WireGuard profile, so Settings deactivates it at once and turns automatic connection off: imported VPNs start disconnected. Connecting a VPN activates the profile without a device. Removing a VPN deletes its profile, keys and credentials. Devices of type `tun` or `wireguard` that NetworkManager does not manage are listed as managed by other apps.

## Mobile hotspot

Set up creates the profile `Hotspot` with `802-11-wireless.mode=ap`, `ipv4.method=shared`, `ipv6.method=ignore`, WPA2 (`wpa-psk`, RSN, CCMP) and `autoconnect=no`, as `nmcli device wifi hotspot` does. Bands are Any, 2.4 GHz (`bg`) and 5 GHz (`a`), offered only when `WirelessCapabilities` reports them; the switch needs the access-point capability and the radio on. NetworkManager shares the primary connection through NAT; when the same adapter is that connection, the page warns that starting the hotspot disconnects it. Editing a running hotspot restarts it. The on or off state is live and never restored at startup; it is excluded from reconciliation like the radio. Connected clients are not listed because NetworkManager does not publish them.

## Permissions

The service calls `GetPermissions`. A `no` disables the control with its reason and rejects the operation; `auth` lets polkit ask when the write is made. Only writes allow interactive authorization, so reads and Bar activation never prompt. An `auth` prompt holds the service's request queue until it is answered or times out after 120 seconds. `nmcli` imports run without a polkit agent, so they need `settings.modify.system` without authentication.

## Reconciliation

NetworkManager profiles, the radio, VPN and hotspot states follow live state.
They are not persisted or restored. Only the connectivity-check preference in
`network.toml` participates in managed reconciliation; see the
[initialization contract](public-api.md#initialization-conflicts-and-scheduling).
A conflicting or invalid saved file requires explicit resolution; failed writes
and readback retain the last valid saved value.

## Proxy

The Proxy page reads and writes GSettings `org.gnome.system.proxy` from `gsettings-desktop-schemas`, which GIO, libproxy, Chromium and Firefox set to "Use system proxy settings" follow. Off is `none`; Detect is `auto` with an empty address (WPAD); Script is `auto` with an `http`, `https` or `file:///` address, entered before Script can be chosen; Manual uses the HTTP, HTTPS and SOCKS hosts and ports and `ignore-hosts`. Servers are `host:port` or `[IPv6]:port`; an empty value clears one. FTP and authentication are left unchanged: GSettings would store a proxy password in plain text.

Proxy tools such as Clash Verge switch this setting routinely, so it follows live GSettings: it is not saved, compared or restored, and the page refreshes every 3 seconds while open. Every write is read back. Terminal programs use `http_proxy` and similar environment variables, which Settings does not change.

## Interfaces

See [public API](public-api.md#network).

## Verification

- `cargo test -p remya_network` uses a fake backend for initialization, both resolutions, failure retention, trial profiles, password validation, adapters and permissions, IP validation and reapply, priority, MAC randomization and iwd, the connectivity check, hotspot setup and toggling, VPN import, connection and removal, and Ethernet; and a GSettings memory backend for proxy modes, servers and hosts.
- `python tests/network_smoke.py [debug|release]` runs the service on private session and system buses against a stub NetworkManager and a keyfile GSettings backend, including the radio, connecting, wrong passwords, conflicts, priority, MAC and addressing readback, the connectivity check, the hotspot, VPN connection, proxy writes, a stopped daemon, and the absence of passwords from every emitted message.
- `../../remya-settings/test/network_test.dart` covers the Known networks, Ethernet IP editor, hotspot setup and VPN pages; `../../remya-settings/test/service_controllers_test.dart` covers failed-write retention and unresolved conflicts in the GUI controller.
- On a real NetworkManager 1.58, a disposable `autoconnect=no` profile was added, changed, re-keyed, put in conflict, resolved and forgotten without activating it, and real state and scans were read. A disposable WireGuard file was imported and removed through the installed service, and the real wired profile, `tailscale0` and the desktop proxy were read without changes.
- **Not tested on a real connection:** turning the radio off, connecting, disconnecting and switching Wi-Fi networks, Ethernet disconnect, changing the addressing of a connected profile, connecting a VPN and starting the hotspot. These would interrupt the workstation's connection or broadcast a network; they have stub verification only. Captive-portal sign-in, more than one Wi-Fi adapter, DNS over TLS and the `iwd` backend have also not been run on real hardware.

## Airplane mode

The Airplane mode page uses the ordinary-user Rust service to set NetworkManager
`WirelessEnabled` and `WwanEnabled`, and every BlueZ `Adapter1.Powered`. On disables
all these radios; Off enables all of them, including radios previously off.
Ethernet is unaffected. Wi-Fi also has an independent switch.
No connection profiles or pairing keys are removed.

Airplane mode is derived from fresh live radio readback, including external
changes; there is no saved airplane preference, startup write or reconciliation
conflict. This is a live-state exclusion. A running BlueZ with no adapters is a
valid empty inventory; an unavailable NetworkManager or BlueZ prevents changes.
Hardware blocks, denied authorization, busy Bluetooth operations, partial writes
and mismatched readback are errors. Every radio shutdown is attempted even when
another fails; successful shutdowns are never rolled back. Refresh displays the
actual state so a failed operation never implies that all radios are off.

The GUI polls while this page is open. CLI/D-Bus use the same controller and
verified writes. Verification: network airplane unit tests, `../../remya-settings/test/airplane_test.dart`
and private system/session-bus `tests/airplane_smoke.py`; no real radios are changed
by these automated checks.

Upstream contracts: [NetworkManager radio properties](https://networkmanager.pages.freedesktop.org/NetworkManager/NetworkManager/gdbus-org.freedesktop.NetworkManager.html)
and [BlueZ adapter power](https://github.com/bluez/bluez/blob/master/doc/org.bluez.Adapter.rst).
