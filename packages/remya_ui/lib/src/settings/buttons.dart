import 'package:material_ui/material_ui.dart';

import 'button_style.dart';

/// Emphasis of a [SettingsButton].
enum SettingsButtonVariant {
  /// Default row action, such as Change or Pin to tray.
  tonal,

  /// The single primary action of a page or row, such as Connect.
  filled,

  /// A destructive or disconnecting action, such as Disconnect.
  destructive,
}

/// The solid MD3E button used for actions in settings rows.
class SettingsButton extends StatelessWidget {
  const SettingsButton({
    super.key,
    required this.label,
    required this.onPressed,
    this.icon,
    this.variant = .tonal,
  });

  final String label;
  final VoidCallback? onPressed;
  final Widget? icon;
  final SettingsButtonVariant variant;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final (background, foreground) = switch (variant) {
      .tonal => (colors.secondaryContainer, colors.onSecondaryContainer),
      .filled => (colors.primary, colors.onPrimary),
      .destructive => (colors.errorContainer, colors.onErrorContainer),
    };
    return FilledButton(
      onPressed: onPressed,
      style: settingsSolidStyle(
        colors,
        background: background,
        disabledBackground: colors.onSurface.withValues(alpha: .12),
        foreground: foreground,
        height: settingsControlHeight,
        padding: .fromLTRB(icon == null ? 16 : 12, 0, 16, 0),
        radius: .circular(settingsControlHeight / 2),
        pressedRadius: .circular(12),
      ).copyWith(iconSize: const WidgetStatePropertyAll(18)),
      child: Row(
        mainAxisSize: .min,
        spacing: 8,
        children: [
          ?icon,
          Flexible(child: Text(label, maxLines: 1, overflow: .ellipsis)),
        ],
      ),
    );
  }
}
