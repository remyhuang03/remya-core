import 'package:material_ui/material_ui.dart';

// Shared metrics for settings controls; see remya-core docs/settings-controls.md.
const settingsControlHeight = 40.0;
const settingsMenuItemHeight = 48.0;

/// Solid MD3E button style shared by settings buttons, segments and dropdowns.
ButtonStyle settingsSolidStyle(
  ColorScheme colors, {
  required Color background,
  required Color disabledBackground,
  required Color foreground,
  required double height,
  required EdgeInsetsGeometry padding,
  required BorderRadiusGeometry radius,
  BorderRadiusGeometry? pressedRadius,
}) => ButtonStyle(
  backgroundColor: .resolveWith(
    (states) =>
        states.contains(WidgetState.disabled) ? disabledBackground : background,
  ),
  foregroundColor: .resolveWith(
    (states) => states.contains(WidgetState.disabled)
        ? colors.onSurface.withValues(alpha: .38)
        : foreground,
  ),
  overlayColor: .resolveWith((states) {
    if (states.contains(WidgetState.pressed) ||
        states.contains(WidgetState.focused)) {
      return foreground.withValues(alpha: .12);
    }
    return states.contains(WidgetState.hovered)
        ? foreground.withValues(alpha: .08)
        : null;
  }),
  side: .resolveWith(
    (states) => states.contains(WidgetState.focused)
        ? BorderSide(color: foreground)
        : null,
  ),
  shape: .resolveWith(
    (states) => RoundedRectangleBorder(
      borderRadius: states.contains(WidgetState.pressed)
          ? pressedRadius ?? radius
          : radius,
    ),
  ),
  padding: .all(padding),
  minimumSize: .all(Size(0, height)),
  tapTargetSize: .shrinkWrap,
  visualDensity: .standard,
  splashFactory: NoSplash.splashFactory,
  textStyle: const WidgetStatePropertyAll(
    TextStyle(fontSize: 14, fontWeight: .w500, letterSpacing: 0),
  ),
);
