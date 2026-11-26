# Debugging Remya Core on NixOS

Use a writable Core checkout and put the Remya application checkouts next to
it. The application-specific NixOS instructions describe their local package
overrides and registrations. Run Core first and keep it alive while testing an
application. The commands below use Bash.

## Build the daemon

Enter the shell and build the daemon from the Core checkout.

```sh
nix-shell -I nixpkgs=channel:nixos-unstable
cargo build --locked -p remya_service
```

The shell supplies native build dependencies. Rustup reads the version and
components from [`rust-toolchain.toml`](../rust-toolchain.toml). The first build
downloads the toolchain and Cargo dependencies and writes `target/debug/`.

## Test the D-Bus API

In the same shell, start a private session bus with temporary XDG roots
and call a read-only endpoint. The test starts and stops the daemon itself.

```sh
dbus-run-session --config-file="$REMYA_DBUS_SESSION_CONFIG" -- bash -eu -c '
  scratch=$(mktemp -d)
  export XDG_CONFIG_HOME="$scratch/config" XDG_CACHE_HOME="$scratch/cache"
  export XDG_DATA_HOME="$scratch/data" XDG_STATE_HOME="$scratch/state"
  export GSETTINGS_BACKEND=memory GIO_USE_VFS=local
  target/debug/remyad --debug-root "$scratch" &
  daemon=$!
  cleanup() {
    kill "$daemon" 2>/dev/null || true
    wait "$daemon" 2>/dev/null || true
    rm -rf -- "$scratch"
  }
  trap cleanup EXIT
  gdbus wait --session --timeout 15 top.remya.Core1
  gdbus call --session --dest top.remya.Core1 --object-path /top/remya/Core1 \
    --method top.remya.Core1.Get about
'
```

The reply contains Core's version and `api_version` set to `1`.

## Graphical session

Open terminals from the NixOS Hyprland session, so the daemon and applications
share the display and session bus. Use a guest session with no other Core
daemon active. In each terminal, enter the relevant Nix shell and then set the
same XDG roots before starting the daemon or app. Leave `XDG_RUNTIME_DIR`,
`WAYLAND_DISPLAY`, and `DBUS_SESSION_BUS_ADDRESS` as set by the session.

```sh
debug_home="$HOME/.cache/remya-nixos-debug"
export XDG_CONFIG_HOME="$debug_home/config" XDG_CACHE_HOME="$debug_home/cache"
export XDG_DATA_HOME="$debug_home/data" XDG_STATE_HOME="$debug_home/state"
mkdir -p "$XDG_CONFIG_HOME" "$XDG_CACHE_HOME" "$XDG_DATA_HOME" "$XDG_STATE_HOME"
```

From the Core checkout in its Rust shell, run `target/debug/remyad` in the
foreground. Core stores its state under those XDG roots and reads app
registrations from `$XDG_DATA_HOME/remya/apps/`. Use the configurations and
services in a disposable VM for testing writes. Core can still call the guest
system services and the compositor through their normal interfaces.
Settings can report the display configuration as read-only because the guest's
Hyprland session does not load the scratch configuration under `XDG_CONFIG_HOME`.

The system updater and package-cleanup operations remain Arch-specific. Other
features with fixed system paths or services, including XKB discovery, AppImage
and Flatpak management, privileged actions, and CUPS test pages, need separate
NixOS verification. Keep system package changes in the VM's NixOS configuration.

## Dart CLI

From the Core checkout in a separate terminal, enter the same Flutter shell the
applications use. Install the version declared in [`.fvmrc`](../.fvmrc).

```sh
nix-shell nix/flutter-shell.nix -I nixpkgs=channel:nixos-unstable
fvm install --skip-pub-get
cd tool
fvm dart pub get --enforce-lockfile
fvm dart run remya_cli.dart get about
```

The CLI connects to the running Core daemon on the terminal's session bus.
