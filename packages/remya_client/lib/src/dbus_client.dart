import 'dart:async';
import 'dart:convert';

import 'package:dbus/dbus.dart';

import 'watch_stream.dart';

const settingsBusName = 'top.remya.Core1';
const settingsInterface = 'top.remya.Core1';
const settingsObjectPath = '/top/remya/Core1';

/// Method arity is shared by CLI validation, clients, and server introspection.
const settingsMethods = {
  'Get': 1,
  'Manage': 1,
  'Set': 2,
  'Replace': 3,
  'OpenApp': 2,
  'List': 1,
  'Search': 1,
  'Open': 1,
  'Reload': 0,
  'Resolve': 2,
  'Quit': 0,
  'Close': 0,
  'Restart': 0,
};

typedef SettingsChange = ({String topic, Map<String, dynamic> value});

class SettingsClient {
  /// Creates a client for `top.remya.Core1`.
  ///
  /// When [bus] is supplied, ownership stays with the caller unless [ownsBus]
  /// is `true`, so [close] does not close a shared connection.
  SettingsClient({DBusClient? bus, bool? ownsBus})
    : _bus = _resolveBus(bus, ownsBus),
      _ownsBus = ownsBus ?? (bus == null);

  static DBusClient _resolveBus(DBusClient? bus, bool? ownsBus) {
    if (bus == null && ownsBus == false) {
      throw ArgumentError.value(
        ownsBus,
        'ownsBus',
        'Cannot disown an internally created DBusClient.',
      );
    }
    return bus ?? DBusClient.session();
  }

  final DBusClient _bus;
  final bool _ownsBus;
  final _closeHooks = <Future<void> Function()>{};
  Future<void>? _closing;
  bool _closed = false;
  late final _object = DBusRemoteObject(
    _bus,
    name: settingsBusName,
    path: DBusObjectPath(settingsObjectPath),
  );

  Future<Map<String, dynamic>> call(
    String method, [
    List<String> arguments = const [],
  ]) async {
    if (settingsMethods[method] != arguments.length) {
      throw ArgumentError('Unknown method or invalid arguments: $method');
    }
    final reply = await _object
        .callMethod(
          settingsInterface,
          method,
          arguments.map(DBusString.new),
          replySignature: DBusSignature('s'),
        )
        .timeout(
          Duration(
            seconds:
                method == 'Set' && arguments.firstOrNull == 'installed.c3c4a29c'
                ? 300
                : 30,
          ),
        );
    return jsonDecode(reply.returnValues.single.asString())
        as Map<String, dynamic>;
  }

  /// Opt a feature into persistence after reading its current system snapshot.
  Future<Map<String, dynamic>> manage(String feature) =>
      call('Manage', [feature]);

  Stream<SettingsChange> get changes =>
      DBusRemoteObjectSignalStream(
        object: _object,
        interface: settingsInterface,
        name: 'Changed',
        signature: DBusSignature('ss'),
      ).map(
        (signal) => (
          topic: signal.values[0].asString(),
          value:
              jsonDecode(signal.values[1].asString()) as Map<String, dynamic>,
        ),
      );

  /// Whether a Settings service owner appeared or disappeared on the session bus.
  Stream<bool> get serviceAvailableChanges => _bus.nameOwnerChanged
      .where((event) => event.name == settingsBusName)
      .map((event) => event.newOwner != null);

  Stream<SettingsChange> watch() => watchChangesStream<SettingsChange>(
    source: changes,
    initial: () async => (topic: 'theme', value: await call('Get', ['theme'])),
    onChange: (change) async => change.topic == 'theme'
        ? (topic: 'theme', value: await call('Get', ['theme']))
        : change,
    closeHooks: _closeHooks,
    isClosed: () => _closed,
  );

  /// Stops active watchers on this client and closes the bus only when owned.
  Future<void> close() => _closing ??= _close();

  Future<void> _close() async {
    _closed = true;
    for (final stop in List<Future<void> Function()>.of(_closeHooks)) {
      try {
        await stop();
      } catch (_) {}
    }
    _closeHooks.clear();
    if (_ownsBus) {
      await _bus.close();
    }
  }
}
