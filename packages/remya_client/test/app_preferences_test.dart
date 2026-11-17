import 'dart:async';

import 'package:remya_client/remya_client.dart';
import 'package:test/test.dart';

class _MockSettingsClient extends SettingsClient {
  final changesController = StreamController<SettingsChange>.broadcast();
  final calls = <(String method, List<String> args)>[];
  int closeCount = 0;
  Map<String, dynamic> stored = {'value': 1, 'revision': 'r1'};

  @override
  Stream<SettingsChange> get changes => changesController.stream;

  @override
  Future<Map<String, dynamic>> call(
    String method, [
    List<String> arguments = const [],
  ]) async {
    calls.add((method, arguments));
    if (method == 'Set') {
      stored = {'value': arguments[1], 'revision': 'r2'};
    } else if (method == 'Replace') {
      stored = {'value': arguments[1], 'revision': 'r3'};
    }
    return stored;
  }

  @override
  Future<void> close() async {
    closeCount++;
  }
}

void main() {
  group('AppPreferences', () {
    test('delegates get, set, and replace with app-prefixed keys', () async {
      final client = _MockSettingsClient();
      addTearDown(client.changesController.close);
      final prefs = AppPreferences('remya-bar', client: client);

      expect(await prefs.get('position'), {'value': 1, 'revision': 'r1'});
      await prefs.set('position', 'bottom');
      await prefs.replace('position', {'edge': 'top'}, 'r2');

      expect(client.calls.map((c) => '${c.$1}:${c.$2.join(',')}').toList(), [
        'Get:remya-bar.position',
        'Set:remya-bar.position,"bottom"',
        'Replace:remya-bar.position,{"edge":"top"},r2',
      ]);
    });

    test('close() preserves injected client ownership', () async {
      final client = _MockSettingsClient();
      addTearDown(client.changesController.close);
      final prefs = AppPreferences('remya-bar', client: client);

      await prefs.close();
      expect(client.closeCount, 0);
    });

    test(
      'watch() reads initial value and rereads matching key changes',
      () async {
        final client = _MockSettingsClient();
        addTearDown(client.changesController.close);
        final prefs = AppPreferences('remya-bar', client: client);

        final seen = <Map<String, dynamic>>[];
        final sub = prefs.watch('position').listen(seen.add);
        await Future<void>.delayed(Duration.zero);
        expect(seen, [
          {'value': 1, 'revision': 'r1'},
        ]);

        client.stored = {'value': 'top', 'revision': 'r2'};
        // Unrelated topics should not trigger a reread.
        client.changesController.add((topic: 'remya-bar.other', value: {}));
        await Future<void>.delayed(Duration.zero);
        expect(seen, hasLength(1));

        client.changesController.add((topic: 'remya-bar.position', value: {}));
        await Future<void>.delayed(Duration.zero);
        expect(seen, [
          {'value': 1, 'revision': 'r1'},
          {'value': 'top', 'revision': 'r2'},
        ]);

        await sub.cancel();
      },
    );
  });

  group('ThemeConfig', () {
    test('parses valid config and validates accent hex colors', () {
      final parsed = ThemeConfig.parse(
        'version = 1\n[theme]\nstyle_variant = "vibrant"\ntheme_mode = "dark"\ncontrast = "high"',
      ).withSeed('#12AB34');

      expect(parsed.styleVariant, 'vibrant');
      expect(parsed.mode, 'dark');
      expect(parsed.contrast, MaterialContrast.high);
      expect(parsed.seedColor, '#12AB34');

      expect(ThemeConfig.seedFromAccent({'color': '#abcdef'}), '#abcdef');
      expect(ThemeConfig.seedFromAccent({'color': 'not-hex'}), isNull);
      expect(ThemeConfig.seedFromAccentFile('color = "#336699"'), '#336699');
    });

    test('rejects empty or invalid theme schemas', () {
      expect(() => ThemeConfig.parse('   '), throwsFormatException);
      expect(() => ThemeConfig.fromMap({'version': 2}), throwsFormatException);
      expect(
        () => ThemeConfig.fromMap({
          'version': 1,
          'theme': {'theme_mode': 'sepia'},
        }),
        throwsFormatException,
      );
      expect(
        () => ThemeConfig.fromMap({
          'version': 1,
          'theme': {'contrast': 'ultra'},
        }),
        throwsFormatException,
      );
    });
  });
}
