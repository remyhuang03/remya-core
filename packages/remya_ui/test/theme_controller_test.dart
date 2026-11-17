import 'dart:async';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:remya_client/settings_contract.dart';
import 'package:remya_ui/remya_ui.dart';

class _FakeSettingsClient extends SettingsClient {
  _FakeSettingsClient() {
    _changes = StreamController<SettingsChange>.broadcast(
      onListen: () => listenCount++,
      onCancel: () => cancelCount++,
    );
  }

  late final StreamController<SettingsChange> _changes;
  int listenCount = 0;
  int cancelCount = 0;
  int closeCount = 0;
  int themeGetCount = 0;
  int accentGetCount = 0;

  Future<Map<String, dynamic>> Function()? onGetTheme;
  Future<Map<String, dynamic>> Function()? onGetAccent;

  Map<String, dynamic> themeResponse = {
    'version': 1,
    'theme': {
      'style_variant': 'tonal_spot',
      'theme_mode': 'light',
      'contrast': 'standard',
    },
  };
  Map<String, dynamic> accentResponse = {'color': '#8054C9'};

  @override
  Stream<SettingsChange> get changes => _changes.stream;

  void emit(String topic, Map<String, dynamic> value) {
    _changes.add((topic: topic, value: value));
  }

  void emitError(Object error) {
    _changes.addError(error);
  }

  @override
  Future<Map<String, dynamic>> call(
    String method, [
    List<String> arguments = const [],
  ]) async {
    expect(method, 'Get');
    final key = arguments.single;
    if (key == 'theme') {
      themeGetCount++;
      if (onGetTheme case final handler?) {
        return handler();
      }
      return themeResponse;
    }
    if (key == 'accent') {
      accentGetCount++;
      if (onGetAccent case final handler?) {
        return handler();
      }
      return accentResponse;
    }
    throw ArgumentError('Unexpected key: $key');
  }

  @override
  Future<void> close() async {
    closeCount++;
  }

  Future<void> disposeFake() => _changes.close();
}

Map<String, dynamic> _themeMap({
  String mode = 'light',
  String styleVariant = 'tonal_spot',
  String contrast = 'standard',
}) => {
  'version': 1,
  'theme': {
    'style_variant': styleVariant,
    'theme_mode': mode,
    'contrast': contrast,
  },
};

void main() {
  group('SettingsThemeController.service', () {
    test(
      'start() is idempotent and dispose() preserves injected client ownership',
      () async {
        final client = _FakeSettingsClient();
        addTearDown(client.disposeFake);
        final firstGet = Completer<Map<String, dynamic>>();
        client.onGetTheme = () {
          client.onGetTheme = null;
          return firstGet.future;
        };

        final controller = SettingsThemeController.service(client: client);
        final firstStart = controller.start();
        final secondStart = controller.start();

        expect(client.listenCount, 1);
        expect(client.themeGetCount, 1);

        firstGet.complete(_themeMap(mode: 'dark'));
        await Future.wait([firstStart, secondStart]);

        await controller.start();
        expect(client.listenCount, 1);
        expect(client.themeGetCount, 1);
        expect(controller.theme.mode, ThemeMode.dark);

        controller.dispose();
        await Future<void>.delayed(Duration.zero);
        expect(client.cancelCount, 1);
        expect(client.closeCount, 0);
      },
    );

    test('coalesces burst theme and accent signals arriving while a read is in flight', () async {
      final client = _FakeSettingsClient();
      addTearDown(client.disposeFake);
      final initialThemeGet = Completer<Map<String, dynamic>>();
      var callIndex = 0;
      client.onGetTheme = () {
        callIndex++;
        if (callIndex == 1) return initialThemeGet.future;
        return Future.value(client.themeResponse);
      };

      final controller = SettingsThemeController.service(client: client);
      addTearDown(controller.dispose);

      final startFuture = controller.start();
      expect(client.themeGetCount, 1);

      // Emit a burst of signals while the initial read is still suspended.
      client.themeResponse = _themeMap(
        mode: 'dark',
        styleVariant: 'vibrant',
        contrast: 'high',
      );
      client.accentResponse = {'color': '#11AA77'};
      client.emit('theme', _themeMap(mode: 'light'));
      client.emit('accent', {'color': '#223344'});
      client.emit('theme', client.themeResponse);
      client.emit('accent', client.accentResponse);
      await Future<void>.delayed(Duration.zero);

      // Complete the stale initial read with the old light theme.
      initialThemeGet.complete(_themeMap(mode: 'light'));
      await startFuture;
      // Let the dirty follow-up reload finish.
      await Future<void>.delayed(Duration.zero);

      // Initial read + 1 coalesced trailing read (not 1 per signal).
      expect(client.themeGetCount, 2);
      expect(controller.theme.mode, ThemeMode.dark);
      expect(controller.theme.styleVariant, DynamicSchemeVariant.vibrant);
      expect(controller.theme.contrast, MaterialContrast.high);
      expect(controller.theme.seedColor, const Color(0xFF11AA77));
      expect(controller.error, isNull);
    });

    test('discards stale theme/accent combination when signal arrives mid-reload', () async {
      final client = _FakeSettingsClient();
      addTearDown(client.disposeFake);
      final controller = SettingsThemeController.service(client: client);
      addTearDown(controller.dispose);
      await controller.start();

      final staleAccentGet = Completer<Map<String, dynamic>>();
      var accentCalls = 0;
      client.onGetAccent = () {
        accentCalls++;
        if (accentCalls == 1) return staleAccentGet.future;
        return Future.value(client.accentResponse);
      };

      // Start a reload that receives old 'light' theme, then stalls on accent.
      final reloadFuture = controller.reload();
      await Future<void>.delayed(Duration.zero);
      expect(accentCalls, 1);

      // Backend transitions to dark + #00BBCC while accent read #1 is pending.
      client.themeResponse = _themeMap(mode: 'dark');
      client.accentResponse = {'color': '#00BBCC'};
      client.emit('accent', client.accentResponse);
      await Future<void>.delayed(Duration.zero);

      var sawIntermediateLight = false;
      controller.addListener(() {
        if (controller.theme.seedColor == const Color(0xFF112233)) {
          sawIntermediateLight = true;
        }
      });

      staleAccentGet.complete({'color': '#112233'});
      await reloadFuture;
      await Future<void>.delayed(Duration.zero);

      expect(sawIntermediateLight, isFalse);
      expect(controller.theme.mode, ThemeMode.dark);
      expect(controller.theme.seedColor, const Color(0xFF00BBCC));
    });

    test(
      'preserves last valid theme on read failure and clears error on recovery',
      () async {
        final client = _FakeSettingsClient();
        addTearDown(client.disposeFake);
        client.themeResponse = _themeMap(
          mode: 'dark',
          styleVariant: 'fidelity',
        );
        client.accentResponse = {'color': '#335577'};

        final controller = SettingsThemeController.service(client: client);
        addTearDown(controller.dispose);
        await controller.start();

        expect(controller.theme.mode, ThemeMode.dark);
        expect(controller.theme.seedColor, const Color(0xFF335577));
        expect(controller.error, isNull);

        client.onGetTheme = () => Future.error(StateError('daemon busy'));
        await controller.reload();

        expect(controller.error, contains('Could not read the settings theme'));
        expect(controller.theme.mode, ThemeMode.dark);
        expect(controller.theme.styleVariant, DynamicSchemeVariant.fidelity);
        expect(controller.theme.seedColor, const Color(0xFF335577));

        client.onGetTheme = null;
        client.onGetAccent = () => Future.error(StateError('accent offline'));
        await controller.reload();
        expect(controller.error, contains('accent offline'));
        expect(controller.theme.seedColor, const Color(0xFF335577));

        client.onGetAccent = null;
        await controller.reload();
        expect(controller.error, isNull);
        expect(controller.theme.mode, ThemeMode.dark);
      },
    );

    test('handles invalid theme signals and error topic without losing valid theme', () async {
      final client = _FakeSettingsClient();
      addTearDown(client.disposeFake);
      client.themeResponse = _themeMap(mode: 'dark');

      final controller = SettingsThemeController.service(client: client);
      addTearDown(controller.dispose);
      await controller.start();

      client.emit('theme', {'version': 99});
      await Future<void>.delayed(Duration.zero);
      expect(controller.error, contains('Invalid settings update'));
      expect(controller.theme.mode, ThemeMode.dark);

      client.emit('error', {'message': 'Sampling failed'});
      await Future<void>.delayed(Duration.zero);
      expect(controller.error, 'Sampling failed');
      expect(controller.theme.mode, ThemeMode.dark);

      client.emitError(StateError('bus dropped'));
      await Future<void>.delayed(Duration.zero);
      expect(controller.error, contains('bus dropped'));
    });

    test(
      'dispose() during in-flight reload suppresses late notifications',
      () async {
        final client = _FakeSettingsClient();
        addTearDown(client.disposeFake);
        final pending = Completer<Map<String, dynamic>>();
        client.onGetTheme = () => pending.future;

        final controller = SettingsThemeController.service(client: client);
        var notifications = 0;
        controller.addListener(() => notifications++);

        final startFuture = controller.start();
        controller.dispose();
        pending.complete(_themeMap(mode: 'dark'));
        await startFuture;

        expect(notifications, 0);
        expect(client.cancelCount, 1);
      },
    );
  });

  group('SettingsThemeController file mode', () {
    test(
      'idempotent start, coalesced reloads, and last-valid-theme recovery',
      () async {
        final dir = await Directory.systemTemp.createTemp('remya_theme_ctrl_');
        addTearDown(() => dir.delete(recursive: true));
        final themeFile = File('${dir.path}/theme.toml');
        final accentFile = File('${dir.path}/accent.toml');
        await themeFile.writeAsString(
          'version = 1\n[theme]\ntheme_mode = "dark"\n',
        );
        await accentFile.writeAsString('color = "#4488CC"\n');

        var resolverFailed = true;
        final controller = SettingsThemeController(() {
          if (resolverFailed) throw StateError('theme path unavailable');
          return themeFile;
        });
        addTearDown(controller.dispose);

        await Future.wait([controller.start(), controller.start()]);
        expect(controller.error, contains('theme path unavailable'));
        resolverFailed = false;
        await controller.reload();
        expect(controller.theme.mode, ThemeMode.dark);
        expect(controller.theme.seedColor, const Color(0xFF4488CC));
        expect(controller.error, isNull);

        await themeFile.writeAsString('not valid toml = [');
        await controller.reload();
        expect(controller.error, contains('Could not read the settings theme'));
        expect(controller.theme.mode, ThemeMode.dark);
        expect(controller.theme.seedColor, const Color(0xFF4488CC));

        await themeFile.writeAsString(
          'version = 1\n[theme]\ntheme_mode = "light"\n',
        );
        await controller.reload();
        expect(controller.error, isNull);
        expect(controller.theme.mode, ThemeMode.light);
      },
    );
  });
}
