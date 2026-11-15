import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:remya_ui/settings_ui.dart';

RemyaSettingsPage _leaf(String id, {RemyaSettingsCategory? category}) =>
    RemyaSettingsPage(
      id: id,
      category: category,
      title: 'Title $id',
      icon: Icons.tune_rounded,
      builder: (_) => Text('Body $id'),
    );

final _pages = [
  _leaf('layout'),
  RemyaSettingsPage(
    id: 'alerts',
    title: 'Alerts',
    icon: Icons.notifications_rounded,
    children: [_leaf('popups'), _leaf('schedule')],
  ),
];

final _footer = [
  RemyaSettingsPage(
    id: 'about',
    title: 'About',
    icon: Icons.info_outline_rounded,
    tabs: false,
    children: [_leaf('about-page'), _leaf('licenses')],
  ),
];

Future<List<String>> _show(WidgetTester tester, String selected) async {
  final picked = <String>[];
  await tester.pumpWidget(
    MaterialApp(
      home: RemyaSettingsScaffold(
        pages: _pages,
        footerPages: _footer,
        selected: selected,
        onSelected: picked.add,
      ),
    ),
  );
  return picked;
}

void main() {
  testWidgets('sidebar groups interleaved pages under their category titles', (
    tester,
  ) async {
    tester.view.devicePixelRatio = 1;
    tester.view.physicalSize = const Size(1200, 800);
    addTearDown(tester.view.resetDevicePixelRatio);
    addTearDown(tester.view.resetPhysicalSize);
    await tester.pumpWidget(
      MaterialApp(
        home: RemyaSettingsScaffold(
          pages: [
            _leaf('layout', category: .personalization),
            _leaf('alerts', category: .system),
            _leaf('theme', category: .personalization),
            _leaf('account', category: .privacy),
          ],
          selected: 'alerts',
          onSelected: (_) {},
        ),
      ),
    );

    final personalization = find.byKey(
      const ValueKey('nav-group-personalization'),
    );
    final system = find.byKey(const ValueKey('nav-group-system'));
    final layout = find.byKey(const ValueKey('nav-layout'));
    final theme = find.byKey(const ValueKey('nav-theme'));
    final alerts = find.byKey(const ValueKey('nav-alerts'));
    expect(
      find.descendant(of: personalization, matching: layout),
      findsOneWidget,
    );
    expect(
      find.descendant(of: personalization, matching: theme),
      findsOneWidget,
    );
    expect(find.descendant(of: system, matching: alerts), findsOneWidget);
    expect(tester.getTopLeft(theme).dy, lessThan(tester.getTopLeft(alerts).dy));
    expect(find.text('Personalization'), findsOneWidget);
    expect(find.text('Device & system'), findsOneWidget);
    expect(find.text('Accounts & privacy'), findsOneWidget);
    expect(find.text('Body alerts'), findsOneWidget);

    tester.view.physicalSize = const Size(780, 800);
    await tester.pumpAndSettle();
    expect(find.text('Personalization'), findsNothing);
    expect(
      find.descendant(of: personalization, matching: theme),
      findsOneWidget,
    );
    expect(tester.getTopLeft(theme).dy, lessThan(tester.getTopLeft(alerts).dy));
  });

  testWidgets('a parent shows its first child under the parent title', (
    tester,
  ) async {
    final picked = await _show(tester, 'alerts');
    expect(find.text('Body popups'), findsOneWidget);
    expect(find.text('Alerts'), findsWidgets);
    await tester.tap(find.byKey(const ValueKey('nav-page-schedule')));
    expect(picked, ['schedule']);
  });

  testWidgets('hidden subpages show their own title without tabs', (
    tester,
  ) async {
    await _show(tester, 'licenses');
    expect(find.text('Body licenses'), findsOneWidget);
    expect(find.text('Title licenses'), findsOneWidget);
    expect(find.byKey(const ValueKey('nav-page-about-page')), findsNothing);
  });

  testWidgets('an unknown id falls back to the first page', (tester) async {
    await _show(tester, 'missing');
    expect(find.text('Body layout'), findsOneWidget);
  });
}
