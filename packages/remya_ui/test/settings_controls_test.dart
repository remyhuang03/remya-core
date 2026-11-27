import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:remya_ui/settings_ui.dart';

Widget _app(Widget child) => MaterialApp(
  home: Scaffold(body: Center(child: child)),
);

void main() {
  testWidgets('settings controls share the 40 px height', (tester) async {
    await tester.pumpWidget(
      _app(
        Column(
          mainAxisSize: .min,
          children: [
            SettingsButton(label: 'Change', onPressed: () {}),
            SettingsSegmentedButton<int>.choices(
              choices: const {1: 'One', 2: 'Two'},
              selected: 1,
              onSelected: (_) {},
            ),
            SettingsDropdown<int>(
              choices: const {1: 'One', 2: 'Two', 3: 'Three', 4: 'Four'},
              selected: 1,
              onSelected: (_) {},
            ),
          ],
        ),
      ),
    );
    for (final button in tester.widgetList(find.byType(FilledButton))) {
      expect(tester.getSize(find.byWidget(button)).height, 40);
    }
  });

  testWidgets('dropdown menu items are 48 px tall', (tester) async {
    await tester.pumpWidget(
      _app(
        SettingsDropdown<int>(
          choices: const {1: 'One', 2: 'Two', 3: 'Three', 4: 'Four'},
          selected: 1,
          onSelected: (_) {},
        ),
      ),
    );
    await tester.tap(find.byType(FilledButton));
    await tester.pumpAndSettle();
    expect(
      tester.getSize(find.widgetWithText(MenuItemButton, 'Two')).height,
      48,
    );
  });

  testWidgets('slider writes once per drag', (tester) async {
    final writes = <double>[];
    await tester.pumpWidget(
      _app(
        SizedBox(
          width: 300,
          child: SettingsSlider(value: 0, max: 10, onChanged: writes.add),
        ),
      ),
    );
    await tester.drag(find.byType(Slider), const Offset(120, 0));
    await tester.pumpAndSettle();
    expect(writes, hasLength(1));
    expect(writes.single, greaterThan(0));
    // Keeps the released value until the saved value reads back.
    expect(tester.widget<Slider>(find.byType(Slider)).value, writes.single);
  });
}
