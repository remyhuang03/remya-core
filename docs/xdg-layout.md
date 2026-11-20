# XDG layout and Core installation

Only absolute XDG roots are used. Missing or relative values fall back to absolute
`HOME`: `.config`, `.cache`, `.local/share`, and `.local/state` respectively.

| Owned files | Release root | `--debug-root <repo>` root |
| --- | --- | --- |
| System feature snapshots | `$XDG_CONFIG_HOME/remya/system/` | `<repo>/config/system/` |
| Application preferences | `$XDG_CONFIG_HOME/remya/<config>/settings.toml` | `<repo>/config/<config>/settings.toml` |
| Shared Remya preferences | `$XDG_CONFIG_HOME/remya/shared/` | `<repo>/config/shared/` |
| Cache | `$XDG_CACHE_HOME/remya/core/` | `<repo>/config/cache/` |
| Data | `$XDG_DATA_HOME/remya/core/` | `<repo>/config/data/` |
| State | `$XDG_STATE_HOME/remya/core/` | `<repo>/config/state/` |

System filenames remain `display.toml`, `night-light.toml`, `input.toml`,
`desktop.toml`, `appearance.toml` (desktop light/dark snapshot), `themes.toml`, `text-size.toml`, `language.toml`, `hostname.toml`,
`default-apps.toml`, `startup.toml`, `network.toml`, `background.toml`, `power.json`,
`power.runtime.json`, `cameras.toml`, `date-time.toml`, `cleanup.toml`,
`appimages.toml`, `updates.json`, and the user-selected `avatar.png`.
Shared files are `theme.toml` (version and `[theme]`, see
`config/shared/theme.example.toml`), `accent.toml` (wallpaper accent), and `ai.toml`
(custom AI endpoints, no application-specific preference table).

`google-oauth-client.json` is the optional Google Desktop OAuth client
configuration in the shared root. Install it from the Google Cloud console
with mode `0600`. Refresh tokens and granted account metadata live in the
user's Secret Service keyring, together with the Google avatar sync preference.
See [Google account](google-account.md).

Cache contains `models-dev.json`, `background-previews/`, and any synced
profile photo as `google-avatar-<account hash>.png`. Imported wallpapers
live in data `backgrounds/`. Battery history is state `power.sqlite3`, with SQLite
WAL/SHM files beside it. Generated debug files are ignored by Git.
Camera preview frames are process-local memory owned by the Settings GUI; the
optional Core camera preview library writes no configuration, data or cache.

Hyprland paths remain under `$XDG_CONFIG_HOME/hypr`, including
`remya-settings/*.lua`, `monitors.lua`, `hyprsunset.conf`, `hypridle.conf`, and
`hyprpaper.conf`. The touchscreen display preference shares `input.toml` and
`remya-settings/input.lua` with mouse, touchpad and shortcut preferences.
Debug roots do not redirect host integration files. Custom AI
keys still use Secret Service with the unchanged `top.remya.settings` application attribute value;
CodexBar retains its own configuration and credential storage.

## Application registrations

App-owned schemas are installed in `$XDG_DATA_HOME/remya/apps/<id>.json` or
`<XDG_DATA_DIRS entry>/remya/apps/<id>.json`. Absolute data roots are searched in
XDG order; relative entries are ignored and the first filename wins, including
invalid registrations. Desktop actions use the same roots at
`applications/<desktop-id>.desktop`. Debug roots redirect app preference storage
only; registrations and desktop files still use the XDG data roots. Set temporary
XDG variables to isolate discovery during tests.

Registration ids/configs are validated before any path is constructed. Missing
keys use registered defaults; reads and discovery never create settings files.
Removed apps leave their preference files untouched and become unavailable after
rescan. See [app preferences](app-preferences.md#application-owned-preferences).

## Managed on demand

A system feature is managed iff its saved configuration exists. Refreshing an
unmanaged feature only observes the system and updates its in-memory snapshot;
it writes no snapshot and reports no conflict. `remya manage <feature>` /
`Manage(s feature)` atomically saves the current complete snapshot. Successful
Set or Resolve starts management too. Managed features preserve conflict rules
and detection/persistence errors. Live-state exclusions are unchanged. The
Remya-owned shared theme may initialize on first read.

## Build and install

```sh
cargo build --release -p remya_service -p remya_updates
(cd packages/remya_client && fvm dart pub get)
(cd tool && fvm dart pub get)
fvm dart compile exe tool/remya_cli.dart -o target/release/remya
python tool/install.py
```

The installer stages only `remyad`, `remya-core-update`, and `remya` from
`target/release/`, then swaps `$XDG_DATA_HOME/remya/remya_core/` by rename.
It creates `~/.local/bin/remya`, refusing unrelated files or symlinks. It asks a
running installed Core to Quit before replacement; a refused Quit (a system
update is running) aborts the update. Clients may re-activate the old daemon
meanwhile; the swap by rename and the final unit restart still replace it.

D-Bus activation uses `$XDG_DATA_HOME/dbus-1/services/top.remya.Core1.service`
from `packaging/top.remya.Core1.service`, with the installed daemon substituted
for `@REMYAD@` and `SystemdService=top.remya.core.service`. The user unit at
`$XDG_DATA_HOME/systemd/user/top.remya.core.service` uses `Type=dbus`,
`BusName=top.remya.Core1`, and `WantedBy=graphical-session.target`; the installer
reloads systemd, enables and restarts it. It installs no Settings GUI files and
never modifies the old `remya_settings` install.

The optional root updater is installed separately with
`sudo python tool/install_system_updater.py`, using `target/release/remya-core-update`
as its source. Normal Core installation does not request root privileges.

## Moving existing configuration

There is no automatic migration and no old-path fallback. Stop Core before moving
files. The commands below use the default HOME locations; substitute your absolute
XDG roots if customized. Run only the `mv` lines whose source exists, and check for
existing destination files before moving.

```sh
mkdir -p ~/.config/remya/system ~/.config/remya/shared
mkdir -p ~/.cache/remya/core ~/.local/state/remya/core ~/.local/share/remya/core
mv ~/.config/remya/settings/display.toml ~/.config/remya/system/display.toml
mv ~/.config/remya/settings/night-light.toml ~/.config/remya/system/night-light.toml
mv ~/.config/remya/settings/input.toml ~/.config/remya/system/input.toml
mv ~/.config/remya/settings/desktop.toml ~/.config/remya/system/desktop.toml
mv ~/.config/remya/settings/themes.toml ~/.config/remya/system/themes.toml
mv ~/.config/remya/settings/text-size.toml ~/.config/remya/system/text-size.toml
mv ~/.config/remya/settings/language.toml ~/.config/remya/system/language.toml
mv ~/.config/remya/settings/hostname.toml ~/.config/remya/system/hostname.toml
mv ~/.config/remya/settings/default-apps.toml ~/.config/remya/system/default-apps.toml
mv ~/.config/remya/settings/startup.toml ~/.config/remya/system/startup.toml
mv ~/.config/remya/settings/network.toml ~/.config/remya/system/network.toml
mv ~/.config/remya/settings/background.toml ~/.config/remya/system/background.toml
mv ~/.config/remya/settings/power.json ~/.config/remya/system/power.json
mv ~/.config/remya/settings/power.runtime.json ~/.config/remya/system/power.runtime.json
mv ~/.config/remya/settings/cameras.toml ~/.config/remya/system/cameras.toml
mv ~/.config/remya/settings/date-time.toml ~/.config/remya/system/date-time.toml
mv ~/.config/remya/settings/cleanup.toml ~/.config/remya/system/cleanup.toml
mv ~/.config/remya/settings/appimages.toml ~/.config/remya/system/appimages.toml
mv ~/.config/remya/settings/updates.json ~/.config/remya/system/updates.json
mv ~/.config/remya/settings/avatar.png ~/.config/remya/system/avatar.png
mv ~/.config/remya/settings/config.toml ~/.config/remya/shared/theme.toml
mv ~/.config/remya/settings/accent.toml ~/.config/remya/shared/accent.toml
mv ~/.config/remya/settings/remya-ai.toml ~/.config/remya/shared/ai.toml
mv ~/.cache/remya/settings/models-dev.json ~/.cache/remya/core/models-dev.json
mv ~/.cache/remya/settings/background-previews ~/.cache/remya/core/background-previews
mv ~/.local/state/remya/settings/power.sqlite3* ~/.local/state/remya/core/
mv ~/.local/share/remya/settings/backgrounds ~/.local/share/remya/core/backgrounds
```

Before restarting, remove the `[bar]` table from the moved `ai.toml`; that table is
no longer accepted. Theme content is unchanged. App-specific Bar/Launcher files
are read only through an installed app-preferences registration.
