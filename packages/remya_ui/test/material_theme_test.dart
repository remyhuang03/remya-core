import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:remya_client/settings_contract.dart';
import 'package:remya_ui/remya_ui.dart';
import 'package:remya_ui/settings_ui.dart';

class _Core extends SettingsClient {
  final updates = StreamController<SettingsChange>.broadcast();
  MaterialContrast contrast = .standard;
  bool reject = false;
  final writes = <List<String>>[];

  @override
  Stream<SettingsChange> get changes => updates.stream;

  @override
  Future<Map<String, dynamic>> call(
    String method, [
    List<String> arguments = const [],
  ]) async {
    if (method == 'Set') {
      expect(arguments.first, materialContrastSettingId);
      writes.add(arguments);
      if (reject) throw StateError('Cannot write preferences');
      contrast = MaterialContrast.values.byName(arguments.last);
      updates.add((topic: 'theme', value: _theme));
    }
    if (arguments.singleOrNull == 'accent') {
      return {'source': 'custom', 'color': '#8054C9'};
    }
    return _theme;
  }

  Map<String, dynamic> get _theme => ThemeConfig(contrast: contrast).toMap();
}

void main() {
  test(
    'older themes default to standard and contrast reaches both schemes',
    () {
      final old = ThemeConfig.parse(
        'version = 1\n[theme]\ntheme_mode = "dark"',
      );
      expect(old.contrast, MaterialContrast.standard);
      final newConfig = ThemeConfig.parse(
        'version = 1\n[theme]\ncontrast = "high"',
      ).withSeed('#8054C9');
      expect(
        ThemeConfig.fromMap(newConfig.toMap()).contrast,
        MaterialContrast.high,
      );
      for (final brightness in Brightness.values) {
        final themed = RemyaTheme.fromConfig(newConfig).theme(brightness);
        expect(
          themed.colorScheme.primary,
          isNot(const RemyaTheme().theme(brightness).colorScheme.primary),
        );
      }
    },
  );

  testWidgets('contrast shows Core readback and retains it on save failure', (
    tester,
  ) async {
    final core = _Core();
    addTearDown(core.updates.close);
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: RemyaMaterialThemeSettings(client: core),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final dropdown = find.byKey(
      const ValueKey('control-themes.material-contrast'),
    );
    expect(tester.widget<DropdownButton<String>>(dropdown).value, 'standard');

    await tester.ensureVisible(dropdown);
    await tester.tap(dropdown);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Medium').last);
    await tester.pumpAndSettle();
    expect(core.writes.single, [materialContrastSettingId, 'medium']);
    expect(tester.widget<DropdownButton<String>>(dropdown).value, 'medium');

    core.reject = true;
    await tester.tap(dropdown);
    await tester.pumpAndSettle();
    await tester.tap(find.text('High').last);
    await tester.pumpAndSettle();
    expect(
      find.textContaining('Could not save the Material theme'),
      findsOneWidget,
    );
    expect(tester.widget<DropdownButton<String>>(dropdown).value, 'medium');
  });
}
