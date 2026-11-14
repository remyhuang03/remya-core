import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'navigation.dart';

/// One destination of a [RemyaSettingsScaffold]. A page with [children] is a
/// sidebar entry whose children are its subpages: they appear as tabs under
/// its title, or, with [tabs] false, are reached only by selecting their id
/// (such as Licenses from About), and then show their own title.
class RemyaSettingsPage {
  const RemyaSettingsPage({
    required this.id,
    required this.title,
    required this.icon,
    this.builder,
    this.children = const [],
    this.tabs = true,
    this.fill = false,
  });

  final String id;
  final String title;
  final IconData icon;

  /// Builds the page content below the header; null for a page with children.
  final WidgetBuilder? builder;
  final List<RemyaSettingsPage> children;
  final bool tabs;

  /// The content takes the remaining window height and scrolls by itself.
  final bool fill;
}

/// The settings window layout of Remya applications, identical to Remy'a
/// Settings: sidebar, page header with sibling tabs and the content frame.
///
/// [pages] share one rounded sidebar group; [footerPages] (usually About)
/// follow as standalone items. [selected] is the id of any page or subpage;
/// selecting a page with children shows its first child. The scaffold does
/// not own navigation: [onSelected] reports the id to show. [onBack], when
/// set, adds a Back button to the header and the Alt+Left shortcut.
class RemyaSettingsScaffold extends StatelessWidget {
  const RemyaSettingsScaffold({
    super.key,
    required this.pages,
    required this.selected,
    required this.onSelected,
    this.footerPages = const [],
    this.loading = false,
    this.onBack,
    this.top,
    this.bottom,
  });

  final List<RemyaSettingsPage> pages;
  final List<RemyaSettingsPage> footerPages;
  final String selected;
  final ValueChanged<String> onSelected;
  final bool loading;
  final VoidCallback? onBack;
  final Widget? top;
  final Widget? bottom;

  /// The sidebar entry and the shown leaf page for [selected]; an unknown id
  /// falls back to the first page.
  ({RemyaSettingsPage entry, RemyaSettingsPage leaf}) _resolve() {
    final entries = [...pages, ...footerPages];
    for (final entry in entries) {
      if (entry.id == selected) {
        return (
          entry: entry,
          leaf: entry.children.isEmpty ? entry : entry.children.first,
        );
      }
      for (final child in entry.children) {
        if (child.id == selected) return (entry: entry, leaf: child);
      }
    }
    final first = entries.first;
    return (
      entry: first,
      leaf: first.children.isEmpty ? first : first.children.first,
    );
  }

  @override
  Widget build(BuildContext context) {
    final (:entry, :leaf) = _resolve();
    final tabs = entry.tabs && entry.children.length > 1;
    Widget item(RemyaSettingsPage page, bool compact, Widget icon) =>
        RemyaSidebarItem(
          key: ValueKey('nav-${page.id}'),
          title: page.title,
          icon: icon,
          selected: page == entry,
          compact: compact,
          onTap: () => onSelected(page.id),
        );
    return CallbackShortcuts(
      bindings: {
        if (onBack case final onBack?)
          const SingleActivator(LogicalKeyboardKey.arrowLeft, alt: true):
              onBack,
      },
      child: RemyaSettingsLayout(
        loading: loading,
        top: top,
        bottom: bottom,
        sidebar: (compact) => RemyaSidebar(
          compact: compact,
          children: [
            RemyaSidebarGroup(
              compact: compact,
              children: [
                for (final page in pages)
                  item(page, compact, RemyaSidebarIcon(page.icon)),
              ],
            ),
            if (footerPages.isNotEmpty) const SizedBox(height: 6),
            for (final page in footerPages)
              item(page, compact, Icon(page.icon, size: 22)),
          ],
        ),
        content: RemyaContentFrame(
          key: ValueKey('content-${leaf.id}'),
          fill: leaf.fill,
          header: RemyaPageHeader(
            title: tabs ? entry.title : leaf.title,
            onBack: onBack,
            tabs: tabs
                ? RemyaPageTabs(
                    tabs: [
                      for (final child in entry.children)
                        (id: child.id, title: child.title, icon: child.icon),
                    ],
                    selected: leaf.id,
                    onSelected: onSelected,
                  )
                : null,
          ),
          child: Builder(builder: leaf.builder!),
        ),
      ),
    );
  }
}
