# Settings controls

Every Remya settings window builds its rows from `package:remya_ui/settings_ui.dart`.
This document says which control to use for each kind of setting. Follow it for new
settings pages and when touching existing ones. Do not reach for the Material
counterpart when a control below covers the case.

Scope: controls inside settings windows (pages, rows, settings dialogs). App surfaces
that are not settings — bar capsules, OSDs, media seek bars, capture toolbars,
launcher results — keep their own purpose-built widgets. Lists, cards and other
layout patterns are not standardized here yet; keep each app's existing approach
unless `remya_ui` already provides the component.

## Choosing a control

| The setting is… | Use | Notes |
|---|---|---|
| on/off | Material `Switch` as the `SettingRow` control | Applies immediately. No shared wrapper yet. |
| one of 2–3 short options | `SettingsSegmentedButton` | Labels ≤ 16 chars. Optional `SettingsSegment.icon`. |
| one of more or longer options | `SettingsDropdown` | Use `SettingsChoice` when the option list is data-driven; it picks segments or dropdown with the rule above. |
| an exact integer in a small range | `SettingsNumberField` | Stepper with typing; commits on Enter, blur, or a short pause after steps. Width follows the digits of `max`. Use a value button + dialog instead for large or formatted values (bytes) or when a lower value deletes data. |
| an integer where both coarse and exact changes matter | `SettingsSliderTile` | Full-width row: label + `SettingsNumberField`, slider below. |
| a continuous value (brightness, opacity, gain) | `SettingsSlider` | Writes once on release and keeps the released value until it reads back. Use `onPreview` for live feedback while dragging (a value label, a preview, or live hardware like brightness). Wrap in a `SizedBox` for a fixed row width. |
| an action on the row (Change, Connect, Pin, View) | `SettingsButton` | See variants below. |
| a value that opens a picker or dialog (time, lists, location) | `SettingsButton` (tonal) showing the current value | Optional leading icon, e.g. `Icons.schedule_rounded` for time. |
| navigation to a subpage or external page | `SettingsLinkRow` | Chevron, or `external: true` for an open-in-new icon. |

Never use Material `SegmentedButton`, `DropdownButton`, `DropdownButtonFormField`,
`DropdownMenu` or `PopupMenuButton` as a settings value control, and never use
`OutlinedButton` in a settings row.

## Buttons

`SettingsButton` is the only button inside settings rows.

| Variant | Use for | Example |
|---|---|---|
| `tonal` (default) | Ordinary row actions and picker triggers | Change, Pin to tray, Add app, Use location, View |
| `filled` | The one primary action of a row or empty state | Connect, Connect Google Tasks, Reconnect |
| `destructive` | Disconnecting or removing an account, data, or device | Disconnect, Forget network |

At most one `filled` button per row. Pair a `destructive` row action with a
confirmation dialog when it loses data or credentials.

Other buttons keep standard Material widgets, per MD3 dialog and header conventions:

- Dialog actions: `TextButton` for Cancel/secondary, `FilledButton` for the confirming
  action, `FilledButton` with `destructiveFilledButtonStyle` for a destructive confirm.
- Section/group header accessories (`SettingsSectionTitle.trailing`,
  `SettingsGroup.trailing`): `TextButton` / `TextButton.icon` (Reset, Privacy policy).
- Icon-only actions in a row (remove, edit): `IconButton` with a `tooltip`.

## Metrics

Defined once in `remya_ui/lib/src/settings/button_style.dart`; do not override per call.

- Control height: 40 logical px for `SettingsButton`, segments and dropdown triggers.
- Label: 14 px, weight 500.
- `SettingsButton`: fully rounded (radius 20), pressed radius 12, 16 px side padding
  (12 px on the icon side), 18 px icon.
- Segments: 20 px outer radius, 6 px inner radius, 2 px gap.
- Dropdown trigger: 12 px radius. Menu: 20 px radius, 6 px inner padding; items 48 px
  tall, 16 px side padding, 14 px radius, 2 px gap between items.

`remya_ui/test/settings_controls_test.dart` locks the heights.

## Behavior

- Apply changes immediately; no Apply buttons (see each app's AGENTS.md for restart
  notices).
- Sliders and number fields write once per gesture, never per drag frame.
- Disable controls with `onPressed`/`onChanged: null` while a write is in flight or the
  value is unavailable; keep the last valid value visible.
- Keep labels short: a name or status, not an action sentence.
