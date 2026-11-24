# Remya Core

Shared backend for the Remya applications on Hyprland/Linux: [Settings](https://github.com/remyhuang03/remya-settings), [Bar](https://github.com/remyhuang03/remya-bar), [Launcher](https://github.com/remyhuang03/remya-launcher) and [Screen Capture](https://github.com/remyhuang03/remya-screen-capture). Each application depends only on Remya Core, so any one of them can be installed on its own.

> [!CAUTION]
> This is a beta release for Arch Linux on Hyprland. Interfaces can change between releases.

## Contents

- `remyad`: the resident Rust daemon on the session bus as `top.remya.Core1`, started by the `top.remya.core.service` user unit or by D-Bus activation.
- `remya`: a CLI with full parity to the D-Bus API.
- `remya-core-update`: the root updater that runs one fixed `pacman -Syu` through pkexec.
- `packages/remya_client`: the pure-Dart client.
- `packages/remya_ui`: the shared Flutter theme and settings UI components.

## Settings tiers

| Tier | Owner | Location |
| --- | --- | --- |
| System | feature crates in `crates/` | `$XDG_CONFIG_HOME/remya/system/` |
| Shared | theme, accent, AI providers | `$XDG_CONFIG_HOME/remya/shared/` |
| App | each app's registration in `remya/apps/<id>.json` | `$XDG_CONFIG_HOME/remya/<app>/settings.toml` |

A system feature becomes managed after an explicit Manage call (including when its page opens in Settings), a successful Set, or a successful Resolve. Installing Bar alone takes over nothing. See [XDG layout](docs/xdg-layout.md), [public API](docs/public-api.md) and [app preferences](docs/app-preferences.md).

## Installation

Install Remya Core before installing any Remya application. Core provides the
`top.remya.Core1` session D-Bus API and the `top.remya.core.service` user unit.

### Arch User Repository

Choose `remya-core` to build from source or `remya-core-bin` for the prebuilt
x86_64 package. For example:

```bash
paru -S remya-core-bin
```

The AUR packages include the privileged system updater and its polkit rule.
See [System updates](docs/updates.md#system-setup) for details.

### Linux x86_64 release archive

Download the archive and `SHA256SUMS` from [GitHub Releases](https://github.com/remyhuang03/remya-core/releases):

```bash
sha256sum -c SHA256SUMS
tar --zstd -xf remya_core-0.2.0-linux-x86_64.tar.zst
cd remya_core-0.2.0-linux-x86_64
python tool/install.py
```

The installer adds the `remya` command, D-Bus activation and a user service that
starts with the graphical session. Enable the optional system updater separately
when installing outside the system package manager. See
[build, installation and migration instructions](docs/xdg-layout.md).

## NixOS development

See [NixOS debugging](docs/nixos-debug.md) for the development shell, a private
D-Bus smoke test, and running Core alongside Remya apps in a Hyprland VM.

## License

[PolyForm Noncommercial License 1.0.0](LICENSE).
