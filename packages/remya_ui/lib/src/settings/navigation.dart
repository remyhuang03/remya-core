import 'package:material_ui/material_ui.dart';

/// Window widths below this use the 80 px icon-only sidebar.
const remyaCompactSidebarWidth = 1000.0;

/// Sidebar beside the content column, as in Remy'a Settings. [sidebar] is
/// built with whether the window is narrow enough for the compact sidebar.
/// [top] and [bottom] are status bars above and below the content; [loading]
/// shows a [RemyaLoadingBar] across the top of the content.
class RemyaSettingsLayout extends StatelessWidget {
  const RemyaSettingsLayout({
    super.key,
    required this.sidebar,
    required this.content,
    this.top,
    this.bottom,
    this.loading = false,
  });
  final Widget Function(bool compact) sidebar;
  final Widget content;
  final Widget? top;
  final Widget? bottom;
  final bool loading;

  @override
  Widget build(BuildContext context) => Scaffold(
    body: SafeArea(
      child: LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth < remyaCompactSidebarWidth;
          return Row(
            children: [
              SizedBox(width: compact ? 80 : 280, child: sidebar(compact)),
              Expanded(
                child: Column(
                  children: [
                    ?top,
                    Expanded(
                      child: RemyaLoadingBar(loading: loading, child: content),
                    ),
                    ?bottom,
                  ],
                ),
              ),
            ],
          );
        },
      ),
    ),
  );
}

/// The only loading indicator for pages and sections: a thin horizontal bar.
/// Without [child] it is the bar itself; with [child] the bar overlays the
/// top of [child] while [loading].
class RemyaLoadingBar extends StatelessWidget {
  const RemyaLoadingBar({super.key, this.loading = true, this.child});
  final bool loading;
  final Widget? child;

  @override
  Widget build(BuildContext context) {
    const bar = LinearProgressIndicator(
      key: ValueKey('remya-loading-bar'),
      semanticsLabel: 'Loading',
    );
    final child = this.child;
    if (child == null) return bar;
    return Stack(
      children: [
        Positioned.fill(child: child),
        if (loading) const Positioned(top: 0, left: 0, right: 0, child: bar),
      ],
    );
  }
}

/// The navigation column: optional [search] above a scrolling list of
/// [RemyaSidebarItem]s and [RemyaSidebarGroup]s.
class RemyaSidebar extends StatelessWidget {
  const RemyaSidebar({
    super.key,
    required this.compact,
    required this.children,
    this.search,
  });
  final bool compact;
  final List<Widget> children;
  final Widget? search;

  @override
  Widget build(BuildContext context) => Material(
    color: Theme.of(context).colorScheme.surfaceContainerLow,
    child: Padding(
      padding: .fromLTRB(compact ? 8 : 16, 24, compact ? 8 : 16, 16),
      child: Column(
        crossAxisAlignment: .stretch,
        children: [
          if (search case final search?) ...[
            search,
            const SizedBox(height: 20),
          ],
          Expanded(child: ListView(children: children)),
        ],
      ),
    ),
  );
}

/// Sidebar items on one rounded container, with an optional [title] above
/// it in the full-width sidebar.
class RemyaSidebarGroup extends StatelessWidget {
  const RemyaSidebarGroup({
    super.key,
    required this.compact,
    required this.children,
    this.title,
  });
  final bool compact;
  final String? title;
  final List<Widget> children;

  @override
  Widget build(BuildContext context) => Semantics(
    container: true,
    explicitChildNodes: true,
    label: compact ? title : null,
    child: Column(
      crossAxisAlignment: .stretch,
      children: [
        if (title case final title?)
          if (compact)
            const SizedBox(height: 14)
          else
            Padding(
              padding: const .fromLTRB(12, 14, 8, 8),
              child: Semantics(
                header: true,
                child: Text(
                  title,
                  style: Theme.of(context).textTheme.labelMedium,
                ),
              ),
            ),
        Material(
          color: Theme.of(context).colorScheme.surfaceContainer,
          borderRadius: .circular(24),
          clipBehavior: .antiAlias,
          child: Column(crossAxisAlignment: .stretch, children: children),
        ),
      ],
    ),
  );
}

/// One sidebar destination: a rounded tile, or a tonal icon button with a
/// tooltip in the compact sidebar. [icon] is usually a [RemyaSidebarIcon]
/// inside groups and a 22 px [Icon] for standalone items such as About.
class RemyaSidebarItem extends StatelessWidget {
  const RemyaSidebarItem({
    super.key,
    required this.title,
    required this.icon,
    required this.selected,
    required this.compact,
    required this.onTap,
  });
  final String title;
  final Widget icon;
  final bool selected;
  final bool compact;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Padding(
      padding: const .symmetric(vertical: 2),
      child: compact
          ? IconButton.filledTonal(
              tooltip: title,
              isSelected: selected,
              style: IconButton.styleFrom(
                backgroundColor: selected
                    ? colors.secondaryContainer
                    : Colors.transparent,
                foregroundColor: selected
                    ? colors.onSecondaryContainer
                    : colors.onSurfaceVariant,
                minimumSize: const Size(48, 48),
              ),
              onPressed: onTap,
              icon: icon,
            )
          : ListTile(
              selected: selected,
              selectedTileColor: colors.secondaryContainer,
              selectedColor: colors.onSecondaryContainer,
              shape: RoundedRectangleBorder(borderRadius: .circular(18)),
              leading: icon,
              title: Text(title, style: const TextStyle(fontSize: 14)),
              contentPadding: const .symmetric(horizontal: 12),
              minTileHeight: 48,
              onTap: onTap,
            ),
    );
  }
}

/// Sidebar colors identify categories across Remya applications.
enum RemyaSettingsCategory {
  connections('Connections', Color(0xffb8d8ff), Color(0xff12385b)),
  personalization('Personalization', Color(0xffffc4df), Color(0xff5b173d)),
  system('Device & system', Color(0xffbdebbd), Color(0xff1b4624)),
  privacy('Accounts & privacy', Color(0xffa8e8ed), Color(0xff004b50)),
  preferences('Language & accessibility', Color(0xffffdda6), Color(0xff593b08));

  const RemyaSettingsCategory(this.title, this.background, this.foreground);

  final String title;
  final Color background;
  final Color foreground;
}

/// A colored circle behind a category or page icon.
class RemyaSidebarIcon extends StatelessWidget {
  const RemyaSidebarIcon(this.icon, {super.key, this.size = 34, this.category});
  final IconData icon;
  final double size;
  final RemyaSettingsCategory? category;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        color: category?.background ?? colors.primaryContainer,
        shape: .circle,
      ),
      child: Icon(
        icon,
        color: category?.foreground ?? colors.onPrimaryContainer,
        size: size * .55,
      ),
    );
  }
}

/// The rounded search field of the sidebar. The clear button calls
/// [onChanged] with an empty string.
class RemyaSearchField extends StatelessWidget {
  const RemyaSearchField({
    super.key,
    required this.controller,
    required this.onChanged,
    this.focusNode,
    this.hintText = 'Find a setting',
  });
  final TextEditingController controller;
  final FocusNode? focusNode;
  final ValueChanged<String> onChanged;
  final String hintText;

  @override
  Widget build(BuildContext context) => ValueListenableBuilder(
    valueListenable: controller,
    builder: (context, value, _) => TextField(
      controller: controller,
      focusNode: focusNode,
      decoration: InputDecoration(
        hintText: hintText,
        prefixIcon: const Icon(Icons.search_rounded, size: 21),
        suffixIcon: value.text.trim().isEmpty
            ? null
            : IconButton(
                tooltip: 'Clear search',
                onPressed: () {
                  controller.clear();
                  onChanged('');
                },
                icon: const Icon(Icons.close_rounded, size: 18),
              ),
        filled: true,
        fillColor: Theme.of(context).colorScheme.surfaceContainerLowest,
        border: const OutlineInputBorder(
          borderSide: .none,
          borderRadius: .all(.circular(18)),
        ),
      ),
      onChanged: (value) => onChanged(value.trim()),
    ),
  );
}

/// Horizontal content padding: 16 px in narrow windows, 32 px otherwise.
double remyaContentPadding(BuildContext context) =>
    MediaQuery.sizeOf(context).width < 600 ? 16 : 32;

/// The page heading: [title], with the sibling-page [tabs] below it and an
/// optional Back button before it. [top] sits above the title, such as the
/// search field of a compact sidebar.
class RemyaPageHeader extends StatelessWidget {
  const RemyaPageHeader({
    super.key,
    required this.title,
    this.tabs,
    this.top,
    this.onBack,
  });
  final String title;
  final Widget? tabs;
  final Widget? top;
  final VoidCallback? onBack;

  @override
  Widget build(BuildContext context) {
    final padding = remyaContentPadding(context);
    final heading = Text(
      title,
      style: Theme.of(context).textTheme.headlineMedium
          ?.copyWith(fontWeight: .w600),
    );
    return Padding(
      padding: .fromLTRB(padding, 20, padding, 16),
      child: Column(
        crossAxisAlignment: .stretch,
        children: [
          if (top case final top?) ...[top, const SizedBox(height: 16)],
          if (onBack case final onBack?)
            Row(
              spacing: 8,
              children: [
                IconButton(
                  tooltip: 'Back',
                  onPressed: onBack,
                  icon: const Icon(Icons.arrow_back_rounded),
                ),
                Expanded(child: heading),
              ],
            )
          else
            heading,
          if (tabs case final tabs?) ...[const SizedBox(height: 16), tabs],
        ],
      ),
    );
  }
}

typedef RemyaPageTab = ({String id, String title, IconData icon});

/// The connected buttons that switch between sibling pages under a heading.
class RemyaPageTabs extends StatelessWidget {
  const RemyaPageTabs({
    super.key,
    required this.tabs,
    required this.selected,
    required this.onSelected,
  });
  final List<RemyaPageTab> tabs;
  final String selected;
  final ValueChanged<String> onSelected;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    // material_ui has no connected button group; use its standard filled buttons.
    return Wrap(
      spacing: 2,
      runSpacing: 6,
      children: [
        for (final (i, tab) in tabs.indexed)
          Semantics(
            selected: tab.id == selected,
            child: FilledButton.icon(
              key: ValueKey('nav-page-${tab.id}'),
              style: FilledButton.styleFrom(
                backgroundColor: tab.id == selected
                    ? colors.primary
                    : colors.secondaryContainer,
                foregroundColor: tab.id == selected
                    ? colors.onPrimary
                    : colors.onSecondaryContainer,
                shape: RoundedRectangleBorder(
                  borderRadius: tab.id == selected
                      ? .circular(24)
                      : .horizontal(
                          left: .circular(i == 0 ? 24 : 8),
                          right: .circular(i == tabs.length - 1 ? 24 : 8),
                        ),
                ),
              ),
              onPressed: () => onSelected(tab.id),
              icon: Icon(tab.icon, size: 18),
              label: Text(tab.title),
            ),
          ),
      ],
    );
  }
}

/// The content column below the sidebar: [header] above [child], which is
/// centered at most 1080 px wide and scrolls with the header. With [fill]
/// the child takes the remaining height instead and scrolls by itself.
class RemyaContentFrame extends StatelessWidget {
  const RemyaContentFrame({
    super.key,
    required this.header,
    required this.child,
    this.fill = false,
  });
  final Widget header;
  final Widget child;
  final bool fill;

  @override
  Widget build(BuildContext context) {
    final padding = remyaContentPadding(context);
    if (fill) {
      return Column(
        children: [
          header,
          Expanded(
            child: Padding(
              padding: .fromLTRB(padding, 8, padding, 24),
              child: child,
            ),
          ),
        ],
      );
    }
    return SelectionArea(
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: .stretch,
          children: [
            header,
            Padding(
              padding: .fromLTRB(padding, 8, padding, 40),
              child: Align(
                alignment: .topCenter,
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 1080),
                  child: child,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}
