import 'dart:async';

import 'package:flex_color_picker/flex_color_picker.dart';
import 'package:material_ui/material_ui.dart';
import 'package:remya_client/settings_contract.dart';

import '../theme.dart';
import 'navigation.dart';
import 'scaffold.dart';
import 'surfaces.dart';

const remyaMaterialThemePageId = 'material-theme';

/// Adds the shared theme to an app's settings window.
RemyaSettingsPage remyaMaterialThemePage({SettingsClient? client}) =>
    RemyaSettingsPage(
      id: remyaMaterialThemePageId,
      title: 'Material theme',
      icon: Icons.palette_rounded,
      category: .personalization,
      builder: (_) => RemyaMaterialThemeSettings(client: client),
    );

/// Edits the theme in Core from any Remya settings window.
class RemyaMaterialThemeSettings extends StatefulWidget {
  const RemyaMaterialThemeSettings({super.key, this.client});

  /// A supplied session connection is borrowed from the caller.
  final SettingsClient? client;

  @override
  State<RemyaMaterialThemeSettings> createState() =>
      _RemyaMaterialThemeSettingsState();
}

class _RemyaMaterialThemeSettingsState
    extends State<RemyaMaterialThemeSettings> {
  late final SettingsClient _client = widget.client ?? SettingsClient();
  late final StreamSubscription<SettingsChange> _changes;
  ThemeConfig? _theme;
  String? _accentSource;
  String? _accentError;
  String? _loadError;
  String? _saveError;
  int _readRevision = 0;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _changes = _client.changes.listen((change) {
      if (const {'theme', 'accent', 'error'}.contains(change.topic)) {
        unawaited(_reload());
      }
    }, onError: (Object error) => unawaited(_reload()));
    unawaited(_reload());
  }

  @override
  void dispose() {
    unawaited(_changes.cancel());
    if (widget.client == null) unawaited(_client.close());
    super.dispose();
  }

  Future<void> _reload() async {
    final revision = ++_readRevision;
    try {
      final replies = await Future.wait([
        _client.call('Get', ['theme']),
        _client.call('Get', ['accent']),
      ]);
      final [theme, accent] = replies;
      final value = ThemeConfig.fromMap(theme)
          .withSeed(ThemeConfig.seedFromAccent(accent));
      final source = accent['source'] as String;
      final accentError = accent['error'] as String?;
      if (!mounted || revision != _readRevision) return;
      setState(() {
        _theme = value;
        _accentSource = source;
        _accentError = accentError;
        _loadError = null;
      });
    } catch (error) {
      if (!mounted || revision != _readRevision) return;
      setState(() => _loadError = 'Could not read the Material theme. $error');
    }
  }

  Future<void> _save(String id, String value) async {
    if (_saving || _loadError != null) return;
    setState(() {
      _saving = true;
      _saveError = null;
    });
    try {
      await _client.call('Set', [id, value]);
      if (mounted) await _reload();
    } catch (error) {
      if (mounted) {
        setState(
          () => _saveError = 'Could not save the Material theme. $error',
        );
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  Future<void> _pickAccent(Color seed) async {
    var picked = seed;
    final confirmed = await ColorPicker(
      color: seed,
      onColorChanged: (color) => picked = color,
      pickersEnabled: const {
        ColorPickerType.primary: true,
        ColorPickerType.accent: false,
        ColorPickerType.wheel: true,
      },
      showColorCode: true,
      colorCodeHasColor: true,
      heading: const Text('Accent color'),
      subheading: const Text('Shades'),
      wheelSubheading: const Text('Shades'),
    ).showPickerDialog(context);
    if (confirmed && mounted) {
      final rgb = picked.toARGB32() & 0xFFFFFF;
      await _save(
        accentColorSettingId,
        '#${rgb.toRadixString(16).padLeft(6, '0').toUpperCase()}',
      );
    }
  }

  SettingRow _choice(
    String id,
    String label,
    String value,
    Map<String, String> choices,
  ) => SettingRow(
    id: id,
    label: SettingLabel(label),
    control: SizedBox(
      width: 200,
      child: Semantics(
        label: label,
        child: InputDecorator(
          decoration: const InputDecoration(
            isDense: true,
            border: OutlineInputBorder(borderRadius: .all(.circular(14))),
          ),
          child: DropdownButtonHideUnderline(
            child: DropdownButton<String>(
              key: ValueKey('control-$id'),
              value: value,
              isExpanded: true,
              isDense: true,
              items: [
                for (final option in choices.entries)
                  DropdownMenuItem(
                    value: option.key,
                    child: Text(option.value),
                  ),
              ],
              onChanged: _saving || _loadError != null
                  ? null
                  : (selected) {
                      if (selected != null && selected != value) {
                        unawaited(_save(id, selected));
                      }
                    },
            ),
          ),
        ),
      ),
    ),
  );

  @override
  Widget build(BuildContext context) {
    final theme = _theme;
    final error = _loadError ?? _saveError;
    return Column(
      crossAxisAlignment: .stretch,
      spacing: 12,
      children: [
        const Text('Changes apply to all Remy\'a apps.'),
        if (error != null)
          SettingsErrorCard(
            message: error,
            actionLabel: _loadError == null ? null : 'Retry',
            onRetry: _saving ? null : _reload,
          ),
        if (theme == null && _loadError == null) const RemyaLoadingBar(),
        if (theme != null)
          SettingsSurface(
            padding: .zero,
            child: settingsRows([
              _choice(
                materialThemeModeSettingId,
                'Theme mode',
                theme.mode,
                const {'light': 'Light', 'dark': 'Dark'},
              ),
              _choice(
                accentSourceSettingId,
                'Accent color source',
                _accentSource!,
                const {'background': 'From background', 'custom': 'Custom'},
              ),
              SettingRow(
                id: accentColorSettingId,
                label: SettingLabel(
                  'Accent color',
                  description: theme.seedColor ?? ThemeConfig.defaultSeedColor,
                ),
                control: _accentSource == 'custom'
                    ? FilledButton.icon(
                        key: const ValueKey('accent-color-button'),
                        onPressed: _saving || _loadError != null
                            ? null
                            : () => _pickAccent(
                                RemyaTheme.seedFromHex(theme.seedColor),
                              ),
                        icon: Icon(
                          Icons.circle,
                          color: RemyaTheme.seedFromHex(theme.seedColor),
                        ),
                        label: const Text('Choose color'),
                      )
                    : null,
              ),
              _choice(
                materialStyleSettingId,
                'Material style variant',
                theme.styleVariant,
                const {
                  'tonal_spot': 'Tonal spot',
                  'fidelity': 'Fidelity',
                  'monochrome': 'Monochrome',
                  'neutral': 'Neutral',
                  'vibrant': 'Vibrant',
                  'expressive': 'Expressive',
                  'content': 'Content',
                  'rainbow': 'Rainbow',
                  'fruit_salad': 'Fruit salad',
                },
              ),
              _choice(
                materialContrastSettingId,
                'Contrast',
                theme.contrast.name,
                const {
                  'standard': 'Standard',
                  'medium': 'Medium',
                  'high': 'High',
                },
              ),
            ]),
          ),
        if (_accentError case final error?)
          SettingsErrorCard(
            message:
                'Could not sample the background. The last color is kept. $error',
            actionLabel: null,
          ),
      ],
    );
  }
}
