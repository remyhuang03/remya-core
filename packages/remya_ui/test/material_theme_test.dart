import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:remya_client/settings_contract.dart';
import 'package:remya_ui/remya_ui.dart';
import 'package:remya_ui/settings_ui.dart';

class _Core extends SettingsClient {
  final updates = StreamController<SettingsChange>.broadcast();
  String mode = 'light';
  String source = 'custom';
  String styleVariant = 'tonal_spot';
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
      writes.add(arguments);
      if (reject) throw StateError('Cannot write preferences');
      switch (arguments.first) {
        case materialThemeModeSettingId:
          mode = arguments.last;
          updates.add((topic: 'theme', value: _theme));
        case accentSourceSettingId:
          source = arguments.last;
          updates.add((topic: 'accent', value: _accent));
        case materialStyleSettingId:
          styleVariant = arguments.last;
          updates.add((topic: 'theme', value: _theme));
        case materialContrastSettingId:
          contrast = MaterialContrast.values.byName(arguments.last);
          updates.add((topic: 'theme', value: _theme));
        default:
          fail('Unexpected setting id: ${arguments.first}');
      }
    }
    if (arguments.singleOrNull == 'accent') {
      return _accent;
    }
    return _theme;
  }

  Map<String, dynamic> get _accent => {'source': source, 'color': '#8054C9'};

  Map<String, dynamic> get _theme => ThemeConfig(
    mode: mode,
    styleVariant: styleVariant,
    contrast: contrast,
  ).toMap();
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

  testWidgets(
    'theme choices render MD3E segments and menu and retain readback on failure',
    (tester) async {
      final core = _Core();
      addTearDown(core.updates.close);
      final appTheme = const RemyaTheme().theme(.light);
      await tester.pumpWidget(
        MaterialApp(
          theme: appTheme,
          home: Scaffold(
            body: SingleChildScrollView(
              child: RemyaMaterialThemeSettings(client: core),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();

      // 2-3 item choices render as solid SettingsSegmentedButton.
      for (final id in [
        materialThemeModeSettingId,
        accentSourceSettingId,
        materialContrastSettingId,
      ]) {
        expect(
          find.descendant(
            of: find.byKey(ValueKey('control-$id')),
            matching: find.byType(SettingsSegmentedButton<String>),
          ),
          findsOneWidget,
        );
      }

      final modeControl = find.byKey(
        const ValueKey('control-$materialThemeModeSettingId'),
      );
      expect(
        tester
            .widget<SettingsSegmentedButton<String>>(
              find.descendant(
                of: modeControl,
                matching: find.byType(SettingsSegmentedButton<String>),
              ),
            )
            .selected,
        'light',
      );
      await tester.tap(find.text('Dark'));
      await tester.pumpAndSettle();
      expect(core.writes.last, [materialThemeModeSettingId, 'dark']);

      await tester.tap(find.text('From background'));
      await tester.pumpAndSettle();
      expect(core.writes.last, [accentSourceSettingId, 'background']);

      final contrastControl = find.byKey(
        const ValueKey('control-$materialContrastSettingId'),
      );
      await tester.ensureVisible(contrastControl);
      await tester.tap(find.text('Medium'));
      await tester.pumpAndSettle();
      expect(core.writes.last, [materialContrastSettingId, 'medium']);
      expect(
        tester
            .widget<SettingsSegmentedButton<String>>(
              find.descendant(
                of: contrastControl,
                matching: find.byType(SettingsSegmentedButton<String>),
              ),
            )
            .selected,
        'medium',
      );

      core.reject = true;
      await tester.tap(find.text('High'));
      await tester.pumpAndSettle();
      expect(
        find.textContaining('Could not save the Material theme'),
        findsOneWidget,
      );
      expect(
        tester
            .widget<SettingsSegmentedButton<String>>(
              find.descendant(
                of: contrastControl,
                matching: find.byType(SettingsSegmentedButton<String>),
              ),
            )
            .selected,
        'medium',
      );
      core.reject = false;

      // Multi-option Material style variant uses SettingsDropdown (MenuAnchor)
      // with MD3E container radius/padding and secondaryContainer selected pill.
      final styleControl = find.byKey(
        const ValueKey('control-$materialStyleSettingId'),
      );
      expect(
        find.descendant(
          of: styleControl,
          matching: find.byType(SettingsDropdown<String>),
        ),
        findsOneWidget,
      );
      expect(
        find.descendant(of: styleControl, matching: find.byType(MenuAnchor)),
        findsOneWidget,
      );
      expect(
        appTheme.menuTheme.style?.shape?.resolve(const {}),
        const RoundedRectangleBorder(borderRadius: .all(.circular(18))),
      );
      expect(
        appTheme.menuTheme.style?.padding?.resolve(const {}),
        const EdgeInsets.all(6),
      );

      await tester.ensureVisible(styleControl);
      await tester.tap(styleControl);
      await tester.pumpAndSettle();

      final selectedItem = tester.widget<MenuItemButton>(
        find.ancestor(
          of: find.text('Tonal spot').last,
          matching: find.byType(MenuItemButton),
        ),
      );
      expect(
        selectedItem.statesController?.value,
        contains(WidgetState.selected),
      );
      expect(
        appTheme.menuButtonTheme.style?.backgroundColor?.resolve({
          WidgetState.selected,
        }),
        appTheme.colorScheme.secondaryContainer,
      );

      await tester.tap(find.text('Expressive').last);
      await tester.pumpAndSettle();
      expect(core.writes.last, [materialStyleSettingId, 'expressive']);
      expect(
        tester
            .widget<SettingsDropdown<String>>(
              find.descendant(
                of: styleControl,
                matching: find.byType(SettingsDropdown<String>),
              ),
            )
            .selected,
        'expressive',
      );
    },
  );
}
