# Default apps

The Apps > Default apps page manages XDG file and URI associations through the ordinary-user Rust service, with GIO for handler lookup and selection. It requires the same GLib/GIO 2.84 minimum as the service. No new third-party dependency or privileged process is introduced.

Control IDs, queries and grouped writes are defined in the
[Default applications API](public-api.md#default-applications).

Reset clears only the `[Default Applications]` group from the active user `mimeapps.list` files. It covers the generic file and the desktop-specific files selected by `XDG_CURRENT_DESKTOP` in XDG config home and XDG data home's `applications` directory. Other groups, including added and removed associations, are preserved. GIO then resolves defaults using the remaining associations and system files. A type may keep the same handler or become unset.

## Ownership, persistence, and failures

`crates/default_apps` owns discovery, validation, readback, and reconciliation. GIO resolves XDG precedence, including desktop-specific files; Settings validates readable association files before accepting GIO's unset result. It never rewrites desktop entry commands or shells out to launch candidates.

`default-apps.toml` under the [system configuration root](xdg-layout.md) contains version 1 and a MIME-to-desktop-ID table. An empty string records an unset default. Unmanaged reads observe real associations without saving; Manage, Set or Resolve starts management. Startup and page entry compare this saved snapshot against fresh system reads. Newly discovered types stay unmanaged until explicit editing or adopting the system. A read or persistence failure leaves the last valid saved snapshot intact.

Conflicts preserve saved and actual values, with explicit Use system configuration and Apply saved Settings actions. Applying saved Settings fails visibly if an application was removed, ceased declaring support, or the saved value is unset. Restoring an unset value through GIO would reset other associations; choose a handler or adopt the actual system state instead.

All mutations are serialized by the service. Reset requires readable configuration without unresolved conflicts. It checks the targeted files before writing, refuses read-only or symlinked files, and verifies each atomic replacement. If a later replacement or verification fails, it attempts to roll back earlier writes that still match the replacement. Reset saves the resulting GIO readback after the cache refresh. Each write checks the just-read default against the snapshot and verifies the resulting handler. Multiple types do not form a system transaction. A failed group attempts to restore previous handlers in reverse order; a previously unset handler cannot be safely restored this way, so incomplete rollback is reported with the affected types. GIO may also add the chosen handler to the association list; restoring a default does not remove those additional candidate associations. Persistence failure after a system write remains visible with the old saved values and the new system values.

Applications that use internal preferences or custom openers may bypass XDG defaults. This feature cannot force those applications to obey the associations.

## Interfaces and verification

The GUI and pure Dart client use the same `Get`, `Set`, and `Resolve` operations as CLI and D-Bus. See [public API](public-api.md#default-applications) and [XDG layout](xdg-layout.md).

- Rust tests cover first-use failure, no-write initialization, both conflict resolutions, invalid input, configuration failure retention, batch rollback, unset rollback limits, and incorrect readback.
- `../../remya-settings/test/service_controllers_test.dart` covers failed-write retention and unresolved conflicts. The picker, extensions, unified search, and icons were verified during implementation; local presentation checks are not retained.
- `python tests/interface_smoke.py` uses synthetic desktop entries under isolated HOME/XDG roots to test real GIO writes/readback, external conflicts, both resolutions, extension search, browser grouping, reset across user roots, preserved associations, unsafe-file refusal, and CLI/D-Bus parity. It also checks that removed pages cannot be opened.

## Primary references

- [freedesktop.org MIME Applications specification](https://specifications.freedesktop.org/mime-apps/latest/)
- [GIO AppInfo](https://docs.gtk.org/gio/iface.AppInfo.html)
- [GIO set_as_default_for_type](https://docs.gtk.org/gio/method.AppInfo.set_as_default_for_type.html)
