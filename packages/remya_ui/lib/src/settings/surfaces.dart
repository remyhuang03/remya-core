import 'package:material_ui/material_ui.dart';

import 'row_layout.dart';

ButtonStyle destructiveFilledButtonStyle(BuildContext context) {
  final colors = Theme.of(context).colorScheme;
  return FilledButton.styleFrom(
    backgroundColor: colors.error,
    foregroundColor: colors.onError,
  );
}

ButtonStyle destructiveTonalButtonStyle(BuildContext context) {
  final colors = Theme.of(context).colorScheme;
  return FilledButton.styleFrom(
    backgroundColor: colors.errorContainer,
    foregroundColor: colors.onErrorContainer,
  );
}

ButtonStyle destructiveTextButtonStyle(BuildContext context) =>
    TextButton.styleFrom(foregroundColor: Theme.of(context).colorScheme.error);

/// The rounded card that holds one group of settings.
class SettingsSurface extends StatelessWidget {
  const SettingsSurface({
    super.key,
    required this.child,
    this.padding = const .all(20),
  });
  final Widget child;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) => Material(
    color: Theme.of(context).colorScheme.surfaceContainerLowest,
    borderRadius: .circular(24),
    clipBehavior: .antiAlias,
    child: Padding(padding: padding, child: child),
  );
}

/// The heading above a group of settings.
class SettingsSectionTitle extends StatelessWidget {
  const SettingsSectionTitle(this.title, {super.key, this.trailing});
  final String title;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) => Padding(
    padding: const .only(top: 28, bottom: 12),
    child: Row(
      children: [
        Expanded(
          child: Text(title, style: Theme.of(context).textTheme.titleMedium),
        ),
        ?trailing,
      ],
    ),
  );
}

/// Rows of one settings group, separated by inset dividers.
Widget settingsRows(List<Widget> rows) => Column(
  children: [
    for (var i = 0; i < rows.length; i++) ...[
      if (i != 0) const Divider(height: 1, indent: 20, endIndent: 20),
      rows[i],
    ],
  ],
);

/// A titled group: [SettingsSectionTitle] above a [SettingsSurface] of rows.
class SettingsGroup extends StatelessWidget {
  const SettingsGroup({
    super.key,
    required this.title,
    required this.children,
    this.trailing,
  });
  final String title;
  final List<Widget> children;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) => Column(
    crossAxisAlignment: .stretch,
    children: [
      SettingsSectionTitle(title, trailing: trailing),
      SettingsSurface(padding: .zero, child: settingsRows(children)),
    ],
  );
}

/// A page-level failure with its message and a Retry action.
class SettingsErrorCard extends StatelessWidget {
  const SettingsErrorCard({
    super.key,
    required this.message,
    this.onRetry,
    this.actionLabel = 'Retry',
  });
  final String message;

  /// Null while a retry cannot start, such as during another operation.
  final VoidCallback? onRetry;

  /// Null shows the message alone, for errors with no action to offer.
  final String? actionLabel;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Padding(
      padding: const .only(bottom: 12),
      child: SettingsSurface(
        child: SettingsRowLayout(
          left: Row(
            mainAxisSize: .min,
            children: [
              Icon(Icons.error_outline_rounded, color: colors.error),
              const SizedBox(width: 16),
              Flexible(
                child: Semantics(liveRegion: true, child: Text(message)),
              ),
            ],
          ),
          right: switch (actionLabel) {
            final label? => TextButton.icon(
              onPressed: onRetry,
              icon: const Icon(Icons.refresh_rounded),
              label: Text(label),
            ),
            null => const SizedBox.shrink(),
          },
        ),
      ),
    );
  }
}

/// A page's default-collapsed options, one rounded card with its rows.
class AdvancedSection extends StatelessWidget {
  const AdvancedSection({
    super.key,
    required this.storageKey,
    required this.child,
    this.title = 'Advanced',
    this.subtitle,
    this.icon = Icons.tune_rounded,
  });
  final Key storageKey;
  final Widget child;
  final String title;
  final String? subtitle;
  final IconData icon;

  static const _radius = BorderRadius.all(.circular(28));

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    const shape = RoundedRectangleBorder(borderRadius: _radius);
    return Padding(
      padding: const .only(top: 24),
      child: Material(
        color: colors.surfaceContainerLowest,
        borderRadius: _radius,
        clipBehavior: .antiAlias,
        child: ExpansionTile(
          key: storageKey,
          tilePadding: const .symmetric(horizontal: 20, vertical: 6),
          shape: shape,
          collapsedShape: shape,
          leading: Container(
            width: 40,
            height: 40,
            decoration: BoxDecoration(
              color: colors.secondaryContainer,
              shape: .circle,
            ),
            child: Icon(icon, size: 20, color: colors.onSecondaryContainer),
          ),
          title: Text(title, style: Theme.of(context).textTheme.titleMedium),
          subtitle: subtitle == null ? null : Text(subtitle!),
          children: [
            const Divider(height: 1, indent: 20, endIndent: 20),
            child,
          ],
        ),
      ),
    );
  }
}

/// Information on the left, the interactive control on the right; stacked when narrow.
class SettingRow extends StatelessWidget {
  const SettingRow({
    super.key,
    required this.id,
    required this.label,
    this.control,
  });
  final String id;
  final Widget label;
  final Widget? control;

  @override
  Widget build(BuildContext context) => Padding(
    key: ValueKey(id),
    padding: const .symmetric(horizontal: 20, vertical: 18),
    child: control == null
        ? Align(alignment: .centerLeft, heightFactor: 1, child: label)
        : SettingsRowLayout(left: label, right: control!),
  );
}

/// The title and optional description of a [SettingRow].
class SettingLabel extends StatelessWidget {
  const SettingLabel(this.title, {super.key, this.description});
  final String title;
  final String? description;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    return Column(
      crossAxisAlignment: .start,
      children: [
        Text(title, style: text.bodyLarge),
        if (description case final description?) ...[
          const SizedBox(height: 6),
          Text(description, style: text.bodySmall),
        ],
      ],
    );
  }
}

/// A whole-row link: icon, text, then [trailing] and a chevron, or an
/// open-in-new icon when [external] leaves the window. Without [onTap] it is
/// a plain information row.
class SettingsLinkRow extends StatelessWidget {
  const SettingsLinkRow({
    super.key,
    required this.title,
    this.icon,
    this.subtitle,
    this.trailing,
    this.onTap,
    this.external = false,
  });
  final IconData? icon;
  final String title;
  final String? subtitle;
  final Widget? trailing;
  final VoidCallback? onTap;
  final bool external;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    final row = Padding(
      padding: const .symmetric(horizontal: 20, vertical: 16),
      child: SettingsRowLayout(
        left: Row(
          mainAxisSize: .min,
          children: [
            if (icon != null) ...[
              Icon(icon, color: Theme.of(context).colorScheme.primary),
              const SizedBox(width: 16),
            ],
            Flexible(
              child: Column(
                crossAxisAlignment: .start,
                children: [
                  Text(title, style: text.bodyLarge),
                  if (subtitle case final subtitle?) ...[
                    const SizedBox(height: 4),
                    Text(subtitle, style: text.bodySmall),
                  ],
                ],
              ),
            ),
          ],
        ),
        right: Row(
          mainAxisSize: .min,
          spacing: 12,
          children: [
            ?trailing,
            if (onTap != null)
              Icon(
                external
                    ? Icons.open_in_new_rounded
                    : Icons.chevron_right_rounded,
                size: 20,
              ),
          ],
        ),
      ),
    );
    if (onTap == null) return row;
    return Semantics(
      button: true,
      child: InkWell(onTap: onTap, child: row),
    );
  }
}
