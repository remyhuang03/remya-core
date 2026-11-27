import 'dart:async';

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import 'surfaces.dart';

/// A slider that previews while dragging and commits once on release.
class SettingsSlider extends StatefulWidget {
  const SettingsSlider({
    super.key,
    required this.value,
    required this.onChanged,
    this.min = 0,
    this.max = 1,
    this.divisions,
    this.label,
    this.onPreview,
  });

  final double value;
  final double min;
  final double max;
  final int? divisions;
  final String Function(double value)? label;

  /// Called once per gesture with the released value when it changed.
  final ValueChanged<double>? onChanged;

  /// Called with each drag value for live previews, then null on release.
  final ValueChanged<double?>? onPreview;

  @override
  State<SettingsSlider> createState() => _SettingsSliderState();
}

class _SettingsSliderState extends State<SettingsSlider> {
  double? _drag;
  // The released value, shown until the saved value reads back.
  double? _sent;

  @override
  void didUpdateWidget(SettingsSlider oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.value != oldWidget.value) _sent = null;
  }

  @override
  Widget build(BuildContext context) {
    final value = (_drag ?? _sent ?? widget.value).clamp(
      widget.min,
      widget.max,
    );
    final onChanged = widget.onChanged;
    return Slider(
      // ignore: deprecated_member_use
      year2023: false,
      value: value,
      min: widget.min,
      max: widget.max,
      divisions: widget.divisions,
      label: widget.label?.call(value),
      semanticFormatterCallback: widget.label,
      onChanged: onChanged == null
          ? null
          : (next) {
              setState(() => _drag = next);
              widget.onPreview?.call(next);
            },
      onChangeEnd: onChanged == null
          ? null
          : (next) {
              final changed = next != (_sent ?? widget.value);
              setState(() {
                _drag = null;
                if (changed) _sent = next;
              });
              widget.onPreview?.call(null);
              if (changed) onChanged(next);
            },
    );
  }
}

/// A compact number entry with step buttons. Typed values commit on Enter or
/// when focus leaves; out-of-range values clamp, and invalid text reverts.
/// Button presses commit after a short pause so repeated steps write once.
class SettingsNumberField extends StatefulWidget {
  const SettingsNumberField({
    super.key,
    required this.value,
    required this.min,
    required this.max,
    required this.onChanged,
    this.suffix,
    this.onPreview,
  });

  final int value;
  final int min;
  final int max;
  final String? suffix;
  final ValueChanged<int>? onChanged;

  /// Reports the uncommitted value while stepping.
  final ValueChanged<int?>? onPreview;

  @override
  State<SettingsNumberField> createState() => _SettingsNumberFieldState();
}

class _SettingsNumberFieldState extends State<SettingsNumberField> {
  late final _text = TextEditingController(text: '${widget.value}');
  final _focus = FocusNode();
  Timer? _commit;
  int? _pending;
  // The last written value, shown until the saved value reads back.
  int? _sent;

  int get _shown => _pending ?? _sent ?? widget.value;

  @override
  void initState() {
    super.initState();
    _focus.addListener(() {
      if (!_focus.hasFocus) _submit(_text.text);
    });
  }

  @override
  void didUpdateWidget(SettingsNumberField oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.value != oldWidget.value) _sent = null;
    if (!_focus.hasFocus) _text.text = '$_shown';
  }

  @override
  void dispose() {
    _commit?.cancel();
    if (_pending case final value?) widget.onChanged?.call(value);
    _text.dispose();
    _focus.dispose();
    super.dispose();
  }

  void _write(int value) {
    _commit?.cancel();
    final changed = value != (_sent ?? widget.value);
    setState(() {
      _pending = null;
      if (changed) _sent = value;
    });
    widget.onPreview?.call(null);
    _text.text = '$value';
    if (changed) widget.onChanged?.call(value);
  }

  void _submit(String text) {
    final parsed = int.tryParse(text.trim());
    if (parsed == null) {
      _text.text = '$_shown';
      return;
    }
    _write(parsed.clamp(widget.min, widget.max));
  }

  void _step(int delta) {
    final next = (_shown + delta).clamp(widget.min, widget.max);
    setState(() => _pending = next);
    widget.onPreview?.call(next);
    _text.text = '$next';
    _commit?.cancel();
    _commit = Timer(const Duration(milliseconds: 600), () => _write(next));
  }

  @override
  Widget build(BuildContext context) {
    final enabled = widget.onChanged != null;
    final colors = Theme.of(context).colorScheme;
    return Material(
      color: colors.surfaceContainerHigh,
      shape: const StadiumBorder(),
      child: Row(
        mainAxisSize: .min,
        children: [
          IconButton(
            tooltip: 'Decrease',
            onPressed: enabled && _shown > widget.min ? () => _step(-1) : null,
            icon: const Icon(Icons.remove_rounded),
          ),
          SizedBox(
            // ~10 px per digit of max, plus room for the suffix.
            width:
                24.0 +
                10 * '${widget.max}'.length +
                (widget.suffix == null ? 0 : 16),
            child: TextField(
              controller: _text,
              focusNode: _focus,
              enabled: enabled,
              textAlign: .center,
              keyboardType: TextInputType.number,
              inputFormatters: [FilteringTextInputFormatter.digitsOnly],
              // A one- or two-digit field has nothing worth a context menu.
              contextMenuBuilder: null,
              style: Theme.of(context).textTheme.titleMedium,
              decoration: InputDecoration(
                isDense: true,
                border: InputBorder.none,
                suffixText: widget.suffix,
              ),
              onSubmitted: _submit,
            ),
          ),
          IconButton(
            tooltip: 'Increase',
            onPressed: enabled && _shown < widget.max ? () => _step(1) : null,
            icon: const Icon(Icons.add_rounded),
          ),
        ],
      ),
    );
  }
}

/// A numeric setting: a stepper for exact values beside the label, and a
/// full-width slider below for quick, coarse changes. Writes once per gesture.
class SettingsSliderTile extends StatefulWidget {
  const SettingsSliderTile({
    super.key,
    required this.title,
    required this.detail,
    required this.value,
    required this.min,
    required this.max,
    required this.onChanged,
    this.suffix = '%',
  });

  final String title;
  final String detail;
  final int value;
  final int min;
  final int max;
  final String suffix;
  final ValueChanged<int>? onChanged;

  @override
  State<SettingsSliderTile> createState() => _SettingsSliderTileState();
}

class _SettingsSliderTileState extends State<SettingsSliderTile> {
  int? _drag;

  @override
  Widget build(BuildContext context) {
    final value = (_drag ?? widget.value).clamp(widget.min, widget.max);
    final text = Theme.of(context).textTheme;
    final colors = Theme.of(context).colorScheme;
    final onChanged = widget.onChanged;
    String label(int v) => '$v${widget.suffix}';
    return Padding(
      key: ValueKey(widget.title),
      padding: const .fromLTRB(20, 18, 20, 10),
      child: Column(
        crossAxisAlignment: .stretch,
        spacing: 8,
        children: [
          Row(
            spacing: 16,
            children: [
              Expanded(
                child: SettingLabel(widget.title, description: widget.detail),
              ),
              SettingsNumberField(
                value: widget.value,
                min: widget.min,
                max: widget.max,
                suffix: widget.suffix,
                onChanged: onChanged,
                onPreview: (next) => setState(() => _drag = next),
              ),
            ],
          ),
          Row(
            children: [
              Text(label(widget.min), style: text.labelSmall),
              Expanded(
                child: SettingsSlider(
                  value: value.toDouble(),
                  min: widget.min.toDouble(),
                  max: widget.max.toDouble(),
                  divisions: widget.max - widget.min,
                  label: (v) => label(v.round()),
                  onChanged: onChanged == null
                      ? null
                      : (next) => onChanged(next.round()),
                ),
              ),
              Text(
                label(widget.max),
                style: text.labelSmall?.copyWith(
                  color: colors.onSurfaceVariant,
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}
