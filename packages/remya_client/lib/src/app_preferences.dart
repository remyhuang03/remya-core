import 'dart:async';
import 'dart:convert';

import 'dbus_client.dart';
import 'watch_stream.dart';

/// Preferences declared by an installed `remya/apps/<id>.json` registration.
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
  Stream<Map<String, dynamic>> watch(String key) =>
      watchChangesStream<Map<String, dynamic>>(
        source: _client.changes.where((change) => change.topic == '$id.$key'),
        initial: () => get(key),
        onChange: (_) => get(key),
      );

  /// Close the connection only when this helper created it.
  Future<void> close() async {
    if (_ownsClient) await _client.close();
  }
}
