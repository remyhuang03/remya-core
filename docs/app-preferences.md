# Shared appearance preferences

Core owns shared `theme.toml` and `accent.toml`; the Settings GUI and other Remya
apps consume the public API. See [XDG layout](xdg-layout.md) for roots and
[public API](public-api.md) for Get/Set, Manage and conflict handling.

`crates/appearance` contains system detection and application, atomic file updates,
and toolkit environment coordination. `crates/service` owns the shared preference
file and serialization. `packages/remya_client` supplies pure Dart contracts;
shared Flutter theme, identity adapters and settings components live in
`packages/remya_ui`. Each app chooses which shared settings it exposes. App-owned
pages and registrations stay in the app repositories. The shared sidebar uses the same
category order, headings and icon colors across apps. Categorized pages of the
same kind share a single sidebar group. The compact sidebar retains the grouping
and exposes category names to assistive technology.


## Shared theme TOML

Release uses `$XDG_CONFIG_HOME/remya/shared/theme.toml`, with absolute HOME fallback.
`--debug-root <repo>` selects `<repo>/config/shared/theme.toml`.

```toml
version = 1

[theme]
style_variant = "tonal_spot"
theme_mode = "light"
contrast = "standard"
```

Styles are `tonal_spot`, `fidelity`, `monochrome`, `neutral`, `vibrant`, `expressive`,
`content`, `rainbow`, and `fruit_salad`. Theme mode is `light` or `dark`.
Contrast is `standard`, `medium` or `high`, mapped to Material contrast levels
0, 0.5 and 1 in light and dark schemes. Missing fields default to Tonal spot,
Light and Standard. A missing theme file initializes with the detected light or
dark mode when available.
`accent.toml` holds the Material seed separately, keeping the theme schema unchanged.

Invalid files retain the last valid in-memory theme and remain on disk for repair.
Writes use atomic replacement and preserve existing links and permissions.
Theme controls write the same Core-owned preferences in every participating app.
Core emits `Changed("theme", …)` or `Changed("accent", …)` so running apps can
apply edits. The theme file and ordinary theme operations affect Remya apps only.

## System targets

Settings exposes an explicit Apply to system action. It passes the current
Remya light or dark mode to the system appearance backend. Accent, palette style
and contrast apply to Remya apps. A successful action writes the verified desktop
mode to `remya/system/appearance.toml`. Core tracks changes to that snapshot
independently of the shared Remya theme. Use `Resolve appearance system` to
adopt the current desktop mode into the snapshot or `Resolve appearance settings`
to reapply the saved desktop mode.


GNOME uses the interface GSettings color scheme. GTK 3 uses its dark-theme key;
GTK 4 uses `gtk-interface-color-scheme`. Existing light/dark theme variants are
selected only when installed. Qt Quick Material and Universal defaults use
validated environment blocks in fish and Hyprland when those styles are installed.
Other Qt styles and portal availability retain their existing platform behavior.

Environment blocks preserve unrelated content, reject duplicate/incomplete markers,
and undergo syntax validation before application. Appearance updates use a
cross-process lock under `remya/shared/`. The desktop snapshot and toolkit
changes use the same verified transaction and rollback. Debug preference roots do not isolate
explicit host appearance writes; tests use injected temporary contexts.

`system_theme::tests` verifies isolated GNOME and GTK readback, transaction
rollback and validation of the managed desktop snapshot. Run
`python tests/interface_smoke.py --theme` to verify CLI and D-Bus parity,
contrast persistence, desktop isolation and explicit application with a private
session bus and XDG/GSettings roots.


## Application-owned preferences

Each application installs `remya/apps/<id>.json` under an absolute XDG data root.
Lookup order is `$XDG_DATA_HOME` (or `$HOME/.local/share`), then `$XDG_DATA_DIRS`
(default `/usr/local/share:/usr/share`). Relative entries are ignored. The first
filename for an id wins, even if invalid; it does not fall through to a lower
priority registration. The filename stem must match the registration id.

```json
{
  "version": 1,
  "id": "remya-bar",
  "app": "top.remya.bar",
  "title": "Remya Bar",
  "icon": "top.remya.bar",
  "config": "bar",
  "pages": [{"id": "layout", "title": "Layout", "keywords": ["pill", "modules"]}],
  "settings": {
    "layout": {
      "title": "Bar layout", "page": "layout", "keywords": [],
      "default": {"size": 2},
      "schema": {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {"size": {"type": "integer", "minimum": 1}},
        "required": ["size"], "additionalProperties": false
      }
    }
  }
}
```

All shown fields are required. Unknown registration, page and setting fields
reject the whole file. App ids are `[a-z0-9]+(-[a-z0-9]+)*` with at most 64 ASCII
characters; page ids and setting keys use the same grammar without the length
limit. Page ids must be unique and each setting must reference a declared page.
Desktop ids omit `.desktop` and contain only ASCII letters, digits, dots,
underscores and hyphens. Titles and icon names must be nonempty; icons are names,
not paths. Config names use `[a-z0-9-]+`, excluding `system`, `shared`, `core` and
`settings`. Catalog page/setting prefixes are reserved. Duplicate config names
are rejected after the first accepted registration (XDG order, sorted filenames
within a root). A mismatched/duplicate registration id is rejected rather than
claiming another filename's namespace. Errors remain visible in List apps.

Schemas are meta-validated against draft 2020-12, compiled once per registration
load, and checked against their defaults. `$schema`, when provided, must identify
2020-12. The `jsonschema` crate has default features disabled: registration loading
does not fetch HTTP or filesystem schema references. Bundle schemas and local
`$defs`/`$ref` in the registration. Defaults and submitted values must also be
TOML-representable; nested nulls are rejected even when the schema permits them.

Values live in `$XDG_CONFIG_HOME/remya/<config>/settings.toml` (debug root:
`<repo>/config/<config>/settings.toml`), one top-level key per setting. Missing
keys use defaults. Writes preserve other keys and use the existing atomic write
helper. Neither discovery nor reads write application preferences. The daemon
never deletes an unregistered application's file. No app-specific migration or
compatibility fields such as the old Bar sidebar `expected` field exist here.
See [public API](public-api.md#application-preferences) for revisions and deep links.
