import 'dart:convert';

import 'package:toml/toml.dart';

const materialThemeModeSettingId = 'themes.material-theme-mode';
const materialStyleSettingId = 'themes.material-style-variant';
const materialContrastSettingId = 'themes.material-contrast';
const accentSourceSettingId = 'themes.8983f367';
const accentColorSettingId = 'themes.6f67371e';

const applyThemeToSystemSettingId = 'themes.apply-to-system';

const materialThemeSettingIds = {
  materialThemeModeSettingId,
  materialStyleSettingId,
  materialContrastSettingId,
  accentSourceSettingId,
  accentColorSettingId,
};

enum MaterialContrast {
  standard(0),
  medium(0.5),
  high(1);

  const MaterialContrast(this.level);
  final double level;
}

/// Versioned public configuration. Keep Flutter and system backends out of here.
class ThemeConfig {
  const ThemeConfig({
    this.styleVariant = 'tonal_spot',
    this.mode = 'light',
    this.contrast = .standard,
    this.seedColor,
  });

  /// The Material seed when no accent has been chosen.
  static const defaultSeedColor = '#8054C9';

  static const styles = {
    'tonal_spot',
    'fidelity',
    'monochrome',
    'neutral',
    'vibrant',
    'expressive',
    'content',
    'rainbow',
    'fruit_salad',
  };
  final String styleVariant;
  final String mode;
  final MaterialContrast contrast;

  /// `#RRGGBB`, published separately by the `accent` key so `theme` stays version 1.
  final String? seedColor;

  /// Reads the `accent` D-Bus value or the content of `accent.toml`.
  static String? seedFromAccent(Map<String, dynamic> accent) {
    final color = accent['color'];
    return color is String && RegExp(r'^#[0-9A-Fa-f]{6}$').hasMatch(color)
        ? color
        : null;
  }

  static String? seedFromAccentFile(String content) =>
      seedFromAccent(TomlDocument.parse(content).toMap());

  ThemeConfig withSeed(String? seed) => .new(
    styleVariant: styleVariant,
    mode: mode,
    contrast: contrast,
    seedColor: seed,
  );

  factory ThemeConfig.parse(String content) {
    if (content.trim().isEmpty) {
      throw const FormatException('Theme configuration is empty');
    }
    return .fromMap(TomlDocument.parse(content).toMap());
  }

  factory ThemeConfig.fromMap(Map<String, dynamic> map) {
    if (map['version'] is! int ||
        map['version'] != 1 ||
        map.keys.any((key) => !const {'version', 'theme'}.contains(key))) {
      throw const FormatException('Unsupported shared theme schema');
    }
    final theme = map.containsKey('theme') ? map['theme'] : <String, dynamic>{};
    if (theme is! Map ||
        theme.keys.any(
          (key) =>
              !const {'style_variant', 'theme_mode', 'contrast'}.contains(key),
        )) {
      throw const FormatException('Invalid theme table');
    }
    final style = theme.containsKey('style_variant')
        ? theme['style_variant']
        : 'tonal_spot';
    final mode = theme.containsKey('theme_mode')
        ? theme['theme_mode']
        : 'light';
    if (!styles.contains(style) || !const {'light', 'dark'}.contains(mode)) {
      throw const FormatException('Invalid Material style or theme mode');
    }
    final contrastName = theme.containsKey('contrast')
        ? theme['contrast']
        : 'standard';
    final contrast = MaterialContrast.values
        .where((value) => value.name == contrastName)
        .firstOrNull;
    if (contrast == null) throw const FormatException('Invalid theme contrast');
    return .new(
      styleVariant: style as String,
      mode: mode as String,
      contrast: contrast,
    );
  }

  Map<String, dynamic> toMap() => {
    'version': 1,
    'theme': {
      'style_variant': styleVariant,
      'theme_mode': mode,
      'contrast': contrast.name,
    },
  };

  /// Identity for change detection and for passing a theme between processes.
  String toJson() => jsonEncode({...toMap(), 'seed_color': ?seedColor});
}
