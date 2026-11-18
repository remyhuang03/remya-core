import 'package:material_ui/material_ui.dart';

import 'package:remya_client/settings_contract.dart';

export 'package:remya_client/settings_contract.dart' show MaterialContrast;

extension MaterialStyleKey on DynamicSchemeVariant {
  String get configKey => name.replaceAllMapped(
    RegExp('[A-Z]'),
    (match) => '_${match[0]!.toLowerCase()}',
  );
}

/// The shared Material color scheme used by Remya applications.
class RemyaTheme {
  const RemyaTheme({
    this.styleVariant = .tonalSpot,
    this.mode = .light,
    this.contrast = .standard,
    this.seedColor = defaultSeed,
  });

  static const defaultSeed = Color(0xFF8054C9);

  /// Parses `#RRGGBB`; anything else keeps the default seed.
  static Color seedFromHex(String? hex) {
    final value = hex == null
        ? null
        : int.tryParse(hex.substring(1), radix: 16);
    return value == null ? defaultSeed : Color(0xFF000000 | value);
  }

  final DynamicSchemeVariant styleVariant;
  final ThemeMode mode;
  final MaterialContrast contrast;
  final Color seedColor;

  factory RemyaTheme.parse(String content) =>
      .fromConfig(ThemeConfig.parse(content));

  factory RemyaTheme.fromMap(Map<String, dynamic> map) =>
      .fromConfig(ThemeConfig.fromMap(map));

  factory RemyaTheme.fromConfig(ThemeConfig config) => .new(
    styleVariant: DynamicSchemeVariant.values.singleWhere(
      (value) => value.configKey == config.styleVariant,
    ),
    mode: config.mode == 'dark' ? .dark : .light,
    contrast: config.contrast,
    seedColor: seedFromHex(config.seedColor),
  );

  ThemeData theme(Brightness brightness) {
    final colors = ColorScheme.fromSeed(
      seedColor: seedColor,
      brightness: brightness,
      dynamicSchemeVariant: styleVariant,
      contrastLevel: contrast.level,
    );
    // MD3E container & pill-item overrides until material_ui styles Menu natively.
    const menuStyle = MenuStyle(
      padding: WidgetStatePropertyAll(EdgeInsets.all(6)),
      shape: WidgetStatePropertyAll(
        RoundedRectangleBorder(borderRadius: .all(.circular(18))),
      ),
    );
    return .new(
      useMaterial3: true,
      colorScheme: colors,
      iconButtonTheme: const .new(variant: .material3Expressive),
      menuTheme: const MenuThemeData(style: menuStyle),
      menuButtonTheme: MenuButtonThemeData(
        style: ButtonStyle(
          shape: const WidgetStatePropertyAll(StadiumBorder()),
          minimumSize: const WidgetStatePropertyAll(Size(64, 40)),
          padding: const WidgetStatePropertyAll(
            EdgeInsets.symmetric(horizontal: 14, vertical: 8),
          ),
          backgroundColor: .resolveWith((states) {
            if (states.contains(WidgetState.disabled)) return null;
            if (states.contains(WidgetState.selected)) {
              return colors.secondaryContainer;
            }
            return null;
          }),
          foregroundColor: .resolveWith((states) {
            if (states.contains(WidgetState.disabled)) {
              return colors.onSurface.withValues(alpha: .38);
            }
            if (states.contains(WidgetState.selected)) {
              return colors.onSecondaryContainer;
            }
            return colors.onSurface;
          }),
          iconColor: .resolveWith((states) {
            if (states.contains(WidgetState.disabled)) {
              return colors.onSurface.withValues(alpha: .38);
            }
            if (states.contains(WidgetState.selected)) {
              return colors.onSecondaryContainer;
            }
            return colors.onSurfaceVariant;
          }),
          overlayColor: .resolveWith((states) {
            final base = states.contains(WidgetState.selected)
                ? colors.onSecondaryContainer
                : colors.onSurface;
            if (states.contains(WidgetState.pressed) ||
                states.contains(WidgetState.focused)) {
              return base.withValues(alpha: .1);
            }
            if (states.contains(WidgetState.hovered)) {
              return base.withValues(alpha: .08);
            }
            return null;
          }),
        ),
      ),
      dropdownMenuTheme: const DropdownMenuThemeData(
        menuStyle: menuStyle,
        inputDecorationTheme: InputDecorationThemeData(
          isDense: true,
          constraints: BoxConstraints.tightFor(height: 48),
          contentPadding: .symmetric(horizontal: 14),
          border: OutlineInputBorder(borderRadius: .all(.circular(14))),
        ),
      ),
      fontFamilyFallback: const [
        'Noto Sans CJK SC',
        'Noto Sans CJK TC',
        'sans-serif',
      ],
    );
  }
}
