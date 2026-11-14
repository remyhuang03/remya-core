import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';
import 'package:remya_ui/settings_ui.dart';

RemyaSettingsPage _leaf(String id) => RemyaSettingsPage(
  id: id,
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
