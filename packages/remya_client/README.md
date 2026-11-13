# Remya Core client

Pure Dart D-Bus contracts for independent Remya clients, without Flutter.
Import `package:remya_client/remya_client.dart` (or `settings_contract.dart`).
`SettingsClient.manage(feature)` opts a system feature into persistence and
returns its Get payload; `call` exposes the complete Core1 method set.

The canonical interface is `api/core1.xml`. See the repository
[public API](../../docs/public-api.md) and [XDG layout](../../docs/xdg-layout.md).
The daemon, CLI and Settings GUI are built independently. Flutter theme adapters and provider icon assets live in `../remya_ui`.

`openRemyaSettings(debugExecutable: ..., page: ...)` starts the selected GUI
with `--gui <page>`; release calls use Core Open over D-Bus.

Validate with `fvm dart pub get` and `fvm dart analyze`.
