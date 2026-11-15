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

  ThemeData theme(Brightness brightness) => .new(
    useMaterial3: true,
    colorScheme: .fromSeed(
      seedColor: seedColor,
      brightness: brightness,
      dynamicSchemeVariant: styleVariant,
      contrastLevel: contrast.level,
    ),
    iconButtonTheme: const .new(variant: .material3Expressive),
    fontFamilyFallback: const [
      'Noto Sans CJK SC',
      'Noto Sans CJK TC',
      'sans-serif',
    ],
  );
}
