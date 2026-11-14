# Remya Core

Shared backend for the Remya applications on Hyprland/Linux: [Settings](https://github.com/remyhuang03/remya-settings), [Bar](https://github.com/remyhuang03/remya-bar), [Launcher](https://github.com/remyhuang03/remya-launcher) and [Screen Capture](https://github.com/remyhuang03/remya-screen-capture). Each application depends only on Remya Core, so any one of them can be installed on its own.

> [!CAUTION]
> Early development. Interfaces change without compatibility aliases.

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

A system feature is written only after it is first changed or opened in Settings; installing Bar alone takes over nothing. See [XDG layout](docs/xdg-layout.md), [public API](docs/public-api.md) and [app preferences](docs/app-preferences.md).

## Install from source

```bash
python tool/install.py
```

Install Remya Core before any Remya application. Source installs that use system updates also need the root updater once:

```bash
sudo python tool/install_system_updater.py
```

Packages (`remya-core`, `remya-core-bin`, `remya-core-git`) install the updater and its polkit rule themselves.

## License

[PolyForm Noncommercial License 1.0.0](LICENSE).
