# Agent Instructions & Development Guidelines

## Project Scope
- `remya-core` is the shared settings backend for Remya applications on Hyprland/Linux. It provides the resident Rust daemon `remyad`, the `remya` CLI, the versioned session D-Bus API `top.remya.Core1`, the pure-Dart client `packages/remya_client` and the Flutter adapter `packages/remya_ui`.
- Every Remya application (Settings, Bar, Launcher, Screen Capture) depends on remya-core, never on another application. Installing one application must not require or imply installing another.
- Settings are split into three tiers:
  - **System** settings (display, input, network, sound, power, appearance, printers, updates, …) are implemented by the feature crates in `crates/` and stored under `remya/system/`.
  - **Shared** Remya settings (Remya Material theme, wallpaper accent, AI providers) are stored under `remya/shared/`.
  - **App** preferences are declared by each application in a registration file (`remya/apps/<id>.json`) and stored by the generic app-preferences store under `remya/<config>/settings.toml`. remyad must not contain code specific to one application's preferences; add application keys to that application's registration instead.
- Host settings remain unavailable previews unless an integration is explicitly implemented and verified. Keep the integration permissions documented in each feature document under `docs/`; do not activate unrelated previews or modify unrelated desktop settings.
- Maintain the settings catalog (`assets/catalog.json`) and its schema alongside the backend. `availability` and `reason` are the only implementation tracker; record verification evidence in the owning feature document before marking a feature available. Never present sample values as detected device state.
- Write rule documents, including `AGENTS.md`, in English.

## Architecture
- Rust crates live in `crates/<feature>/` under the root `Cargo.toml` workspace. Group backend functionality by feature; create crates only for concrete responsibility or dependency boundaries. `crates/service` is the daemon; do not turn it into a catch-all for feature logic.
- Keep the Flutter-free daemon free of UI concerns. `packages/remya_client` stays pure Dart (no Flutter); `packages/remya_ui` holds the theme adapter, shared identity assets and the app-agnostic settings-window components (`lib/settings_ui.dart`) that give every Remya settings window the Settings look; it can provide reusable editors for shared Remya preferences through the public Dart client. System and app-owned pages, catalog logic and native backend code stay outside the UI package.
- Applications consume the packages through a git dependency pinned to a tag. Local development uses a gitignored `pubspec_overrides.yaml` pointing at a local checkout.
- Bar's candidate workflow instead builds from a sibling Core checkout at a verified, full commit SHA and records that SHA in the candidate metadata.

## Public Interfaces
- The daemon runs as the ordinary user, starts with the graphical session through the `top.remya.core.service` systemd user unit and is D-Bus activatable as `top.remya.Core1`. It owns configuration, reconciliation and CLI/D-Bus dispatch.
- Provide complete operation parity through the CLI and D-Bus API for every implemented feature. Expose unavailable catalog entries for inspection but reject attempts to execute them; never simulate success.
- Extend `crates/service`, the client contract, `packages/remya_client/api/core1.xml`, documentation and parity tests together when adding available operations.
- Remove obsolete configuration formats, CLI/D-Bus operations and client interfaces directly. Do not retain aliases, migration code, compatibility branches, deprecated fallbacks or no-op endpoints. Update every caller in all Remya repositories, tests, fixtures and documentation together.
- System writes requiring elevation run in a separately authorized process over private bounded pipes. Authorization happens only on the first explicit Settings GUI launch and is reused while the daemon stays resident. Never expose arbitrary commands, configurable privileged paths or a public privileged socket. Requests from Bar or other read-mostly clients must never prompt for authorization.
- System package updates run only through a fixed root-owned updater: `/usr/lib/remya-core/remya-core-update` with `packaging/50-top.remya.core.update.rules` from packages, or `/usr/local/libexec/remya-core-update` with a per-user rule from `sudo python tool/install_system_updater.py` for source installs; the packaged path wins when both exist. It accepts only `upgrade [--include-held]` and runs one fixed `pacman -Syu`; never widen its commands, accept paths or packages, or install AUR packages.
- Keep `docs/public-api.md` and `docs/xdg-layout.md` synchronized with the implementation. Use isolated XDG roots and GSettings state for system-write verification.
- Release preferences use `XDG_CONFIG_HOME/remya/{system,shared,<app config>}` with the standard HOME fallback; debug runs started with `--debug-root <repo>` use `<repo>/config/…`. Ignore relative XDG values.

## Managed on Demand and Reconciliation
- A system feature is managed only when its saved configuration file exists. Until then remyad reads live state and writes nothing for it. A feature becomes managed on its first successful Set or Resolve, or when a client calls `Manage(feature)` (the Settings GUI does so when a page opens). Never write first-use snapshots at daemon startup or during periodic refresh.
- For managed features: compare saved values with fresh system readback on refresh; on conflict keep both values, report the conflict and resolve only through an explicit adopt-system or apply-saved choice routed through the owning feature backend with validation, authorization and readback verification. Failures stay visible and never silently resolve a conflict or overwrite the last valid configuration.
- Compare stable preferences, not naturally changing measurements. Airplane mode, the Wi-Fi radio, NetworkManager profiles, the GSettings desktop proxy, VPN and hotspot states, CUPS printers and queues, the set of XDG autostart entries, sound volume, mute, default devices and application routing follow live state and are not persisted or restored.
- Hyprland Lua persistence is limited to the files and options documented in `docs/input.md`, `docs/display` sections of `docs/public-api.md` and the feature documents; refuse to edit a file the strict reader cannot prove safe; risky display changes need the keep-or-revert countdown.

## Toolchains & Dependencies
- Assume fish is the user's default shell. Persist appearance-related environment variables only through fish and Hyprland configuration.
- NetworkManager is the only network backend.
- Target current Arch Linux packages; do not add compatibility layers for older system libraries or services.
- Always invoke Dart as `fvm dart ...`. `.fvmrc` is the source of truth for the Flutter version. Do not change it without explicit instruction.
- Use the root `rust-toolchain.toml` and inherit common crate metadata from the workspace. Commit `.fvmrc`, `pubspec.lock` files and `Cargo.lock`.
- Reuse existing helpers and dependencies; add packages only for concrete requirements.
- This project uses the PolyForm Noncommercial License 1.0.0 (`LICENSE`). Check dependency license compatibility and preserve third-party notices. Prefer permissive licenses; keep LGPL libraries dynamically linked or in separate processes; do not introduce GPL, AGPL, SSPL or other incompatible strong-copyleft code.

## Validation
- Run before presenting changes:
  ```bash
  cargo fmt --all -- --check
  cargo clippy --workspace --all-targets -- -D warnings
  cargo test --workspace
  (cd packages/remya_client && fvm dart analyze)
  (cd packages/remya_ui && fvm flutter analyze)
  git diff --check
  ```
- Keep durable tests only for non-trivial logic and regressions. Validate configuration input, preserve the last valid state on errors and use atomic persistence.
- After every code change, rebuild the release binaries and run `python tool/install.py` on this workstation as the final step, then verify the installed daemon, command, D-Bus activation file and user unit use the new build. Install remya-core before any dependent application. Preserve user configuration and unrelated installations.

## Releases
- Use the user-level `remya-release` skill from [remya-apps-skills](https://github.com/remyhuang03/remya-apps-skills). remya-core is released first at every stage.

## Git Workflow
- Develop and validate on a separate branch named `<type>/<short-description>`.
- Use small, focused stacked commits following Conventional Commits: `<type>(<scope>): <description>` with a lowercase imperative description; mark breaking changes with `!` or a `BREAKING CHANGE:` footer. Types: `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`, `build`, `ci`, `chore`, `revert`.
- Inspect diffs, stage only intended paths and preserve unrelated changes.
- Commit approval is mandatory: show completed changes and validation results before requesting explicit approval to run `git commit`. Approval to commit also authorizes merging ready branches into `main`, deleting fully merged local branches and pushing `main` to `origin`; verify remote refs afterwards.
