import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';

import 'theme.dart';

import 'package:remya_client/settings_contract.dart';

class SettingsThemeController extends ChangeNotifier {
  SettingsThemeController(this.resolveFile)
    : _client = null,
      _ownsClient = false;
  SettingsThemeController.service({SettingsClient? client})
    : resolveFile = null,
      _client = client ?? SettingsClient(),
      _ownsClient = client == null;

  final File Function()? resolveFile;
  final SettingsClient? _client;
  final bool _ownsClient;
  StreamSubscription<SettingsChange>? _changes;
  RemyaTheme theme = const .new();
  String? error;
  String? _content;
  Timer? _timer;
  Future<void>? _startFuture;
  Future<void>? _reloadFuture;
  bool _dirty = false;
  bool _disposed = false;

  /// A missing accent file keeps the default seed.
  Future<String?> _readSeed(File? themeFile) async {
    if (_client case final client?) {
      return ThemeConfig.seedFromAccent(await client.call('Get', ['accent']));
    }
    final file = File('${themeFile!.parent.path}/accent.toml');
    return await file.exists()
        ? ThemeConfig.seedFromAccentFile(await file.readAsString())
        : null;
  }

  /// Starts watching theme changes and polling for updates; idempotent.
  Future<void> start() {
    if (_disposed) return Future<void>.value();
    return _startFuture ??= _start();
  }

  Future<void> _start() async {
    if (_client case final client?) {
      _changes = client.changes.listen(
        (change) {
          if (_disposed) return;
          try {
            if (change.topic == 'theme') {
              ThemeConfig.fromMap(change.value);
              unawaited(reload());
            } else if (change.topic == 'accent') {
              unawaited(reload());
            } else if (change.topic == 'error') {
              error =
                  change.value['message']?.toString() ??
                  'Settings service error';
              notifyListeners();
            }
          } catch (failure) {
            error = 'Invalid settings update: $failure';
            notifyListeners();
          }
        },
        onError: (Object failure) {
          if (!_disposed) {
            error = '$failure';
            notifyListeners();
          }
        },
      );
    }
    await reload();
    if (_disposed) return;
    // Atomic replacements and directories created after startup are both seen.
    _timer ??= Timer.periodic(
      Duration(milliseconds: _client == null ? 500 : 2000),
      (_) => unawaited(reload()),
    );
  }

  /// Reloads theme and accent configuration, coalescing concurrent requests.
  Future<void> reload() {
    if (_disposed) return Future<void>.value();
    _dirty = true;
    return _reloadFuture ??= _runReloadLoop().whenComplete(() {
      _reloadFuture = null;
      if (_dirty && !_disposed) return reload();
    });
  }

  Future<void> _runReloadLoop() async {
    while (_dirty && !_disposed) {
      _dirty = false;
      final previousError = error;
      var changed = false;
      try {
        ThemeConfig? config;
        File? file;
        if (_client case final client?) {
          config = ThemeConfig.fromMap(await client.call('Get', ['theme']));
        } else {
          file = resolveFile!();
          if (await file.exists()) {
            config = ThemeConfig.parse(await file.readAsString());
          }
        }
        if (_disposed || _dirty) continue;
        if (config != null) {
          config = config.withSeed(await _readSeed(file));
          if (_disposed || _dirty) continue;
          final content = config.toJson();
          if (content != _content) {
            theme = RemyaTheme.fromConfig(config);
            _content = content;
            changed = true;
          }
        }
        error = null;
      } catch (failure) {
        if (_disposed || _dirty) continue;
        error = 'Could not read the settings theme: $failure';
      }
      if (changed || previousError != error) notifyListeners();
    }
  }

  @override
  void dispose() {
    _disposed = true;
    _timer?.cancel();
    _timer = null;
    final cancel = _changes?.cancel();
    _changes = null;
    if (_ownsClient) {
      unawaited(_closeClientAfter(cancel));
    } else if (cancel != null) {
      unawaited(cancel.catchError((Object _) {}));
    }
    super.dispose();
  }

  Future<void> _closeClientAfter(Future<void>? cancel) async {
    try {
      await cancel;
    } catch (_) {}
    try {
      await _client!.close();
    } catch (_) {}
  }
}
