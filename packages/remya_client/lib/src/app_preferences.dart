import 'dart:async';
import 'dart:convert';

import 'dbus_client.dart';

/// Preferences declared by an installed remya/apps/<id>.json registration.
class AppPreferences {
  AppPreferences(this.id, {SettingsClient? client})
    : _client = client ?? SettingsClient(),
      _ownsClient = client == null;

  final String id;
  final SettingsClient _client;
  final bool _ownsClient;

  Future<Map<String, dynamic>> get(String key) =>
      _client.call('Get', ['$id.$key']);

  Future<Map<String, dynamic>> set(String key, Object? value) =>
      _client.call('Set', ['$id.$key', jsonEncode(value)]);

  Future<Map<String, dynamic>> replace(
    String key,
    Object? value,
    String revision,
  ) => _client.call('Replace', ['$id.$key', jsonEncode(value), revision]);

  /// Subscribe before reading so a concurrent write cannot be missed.
  Stream<Map<String, dynamic>> watch(String key) async* {
    final pending = StreamController<SettingsChange>();
    final subscription = _client.changes
        .where((change) => change.topic == '$id.$key')
        .listen(pending.add, onError: pending.addError);
    try {
      yield await get(key);
      await for (final _ in pending.stream) {
        yield await get(key);
      }
    } finally {
      await subscription.cancel();
      unawaited(pending.close());
    }
  }

  /// Close the connection only when this helper created it.
  Future<void> close() async {
    if (_ownsClient) await _client.close();
  }
}
