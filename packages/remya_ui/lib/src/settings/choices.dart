import 'package:material_ui/material_ui.dart';

import 'button_style.dart';

/// A single option in a [SettingsSegmentedButton].
class SettingsSegment<T> {
  const SettingsSegment(this.value, this.label, {this.icon});

  final T value;
  final String label;
  final Widget? icon;
}

/// Solid connected single-selection buttons matching MD3E styling.
class SettingsSegmentedButton<T> extends StatelessWidget {
  const SettingsSegmentedButton({
    super.key,
    required this.segments,
    required this.selected,
    required this.onSelected,
    this.allowReselect = false,
    this.expand = false,
  });

  SettingsSegmentedButton.choices({
    super.key,
    required Map<T, String> choices,
    required this.selected,
    required this.onSelected,
    this.allowReselect = false,
    this.expand = false,
  }) : segments = [
         for (final MapEntry(:key, :value) in choices.entries)
           SettingsSegment(key, value),
       ];

  final List<SettingsSegment<T>> segments;
  final T? selected;
  final ValueChanged<T>? onSelected;
  final bool allowReselect;
  final bool expand;

  @override
  Widget build(BuildContext context) => Row(
    mainAxisSize: expand ? .max : .min,
    spacing: 2,
    children: [
      for (final (index, segment) in segments.indexed)
        _buildSegment(index, segment),
    ],
  );

  Widget _buildSegment(int index, SettingsSegment<T> segment) {
    final button = _SegmentButton(
      label: segment.label,
      icon: segment.icon,
      first: index == 0,
      last: index == segments.length - 1,
      selected: segment.value == selected,
      onPressed: onSelected == null
          ? null
          : () {
              if (allowReselect || segment.value != selected) {
                onSelected!(segment.value);
              }
            },
    );
    return expand ? Expanded(child: button) : button;
  }
}

class _SegmentButton extends StatelessWidget {
  const _SegmentButton({
    required this.label,
    required this.icon,
    required this.first,
    required this.last,
    required this.selected,
    required this.onPressed,
  });

  final String label;
  final Widget? icon;
  final bool first;
  final bool last;
  final bool selected;
  final VoidCallback? onPressed;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return Semantics(
      selected: selected,
      inMutuallyExclusiveGroup: true,
      child: FilledButton(
        onPressed: onPressed,
        style: settingsSolidStyle(
          colors,
          background: selected
              ? colors.primary
              : colors.surfaceContainerHighest,
          disabledBackground: selected
              ? colors.onSurface.withValues(alpha: .22)
              : colors.onSurface.withValues(alpha: .08),
          foreground: selected ? colors.onPrimary : colors.onSurfaceVariant,
          height: settingsControlHeight,
          padding: const .symmetric(horizontal: 16),
          radius: BorderRadiusDirectional.horizontal(
            start: Radius.circular(selected || first ? 20 : 6),
            end: Radius.circular(selected || last ? 20 : 6),
          ),
          pressedRadius: .circular(12),
        ),
        child: Row(
          mainAxisSize: .min,
          spacing: 6,
          children: [
            ?icon,
            Flexible(child: Text(label, maxLines: 1, overflow: .ellipsis)),
          ],
        ),
      ),
    );
  }
}

/// An option displayed by [SettingsDropdown].
class SettingsChoiceOption<T> {
  const SettingsChoiceOption({
    required this.value,
    required this.label,
    this.leadingIcon,
  });

  final T value;
  final String label;
  final Widget? leadingIcon;
}

/// A [MenuAnchor]-backed solid dropdown button with MD3E popup menu items.
class SettingsDropdown<T> extends StatefulWidget {
  SettingsDropdown({
    super.key,
    required Map<T, String> choices,
    required this.selected,
    required this.onSelected,
    this.label,
    this.hintText,
    this.leadingIcon,
    this.width,
    this.menuHeight,
    this.fillWidth = false,
    bool includeUnknownSelected = true,
  }) : options = [
         for (final MapEntry(:key, :value) in choices.entries)
           SettingsChoiceOption(value: key, label: value),
         if (includeUnknownSelected &&
             hintText == null &&
             selected is T &&
             !choices.containsKey(selected))
           SettingsChoiceOption(value: selected, label: '$selected'),
       ];

  const SettingsDropdown.options({
    super.key,
    required this.options,
    required this.selected,
    required this.onSelected,
    this.label,
    this.hintText,
    this.leadingIcon,
    this.width,
    this.menuHeight,
    this.fillWidth = false,
  });

  final List<SettingsChoiceOption<T>> options;
  final T? selected;
  final ValueChanged<T>? onSelected;
  final String? label;
  final String? hintText;
  final Widget? leadingIcon;
  final double? width;
  final double? menuHeight;
  final bool fillWidth;

  @override
  State<SettingsDropdown<T>> createState() => _SettingsDropdownState<T>();
}

class _SettingsDropdownState<T> extends State<SettingsDropdown<T>> {
  final _focus = FocusNode();
  final _anchorKey = GlobalKey();
  double? _anchorWidth;

  void _syncAnchorWidth() {
    final renderBox = _anchorKey.currentContext?.findRenderObject();
    if (renderBox is RenderBox && renderBox.hasSize) {
      _anchorWidth = renderBox.size.width;
    }
  }

  @override
  void dispose() {
    _focus.dispose();
    super.dispose();
  }

  SettingsChoiceOption<T>? _selectedOption() {
    for (final option in widget.options) {
      if (option.value == widget.selected) return option;
    }
    return null;
  }

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final options = widget.options;
    final selectedOption = _selectedOption();
    final buttonText =
        selectedOption?.label ??
        widget.hintText ??
        widget.label ??
        (widget.selected == null ? '' : '${widget.selected}');
    final stretch = widget.fillWidth || widget.width != null;
    final minWidth = widget.width ?? _anchorWidth;
    final menuStyle = MenuStyle(
      padding: const WidgetStatePropertyAll(.all(6)),
      shape: const WidgetStatePropertyAll(
        RoundedRectangleBorder(borderRadius: .all(.circular(20))),
      ),
      minimumSize: minWidth == null
          ? null
          : WidgetStatePropertyAll(Size(minWidth, 0)),
      maximumSize: widget.menuHeight == null
          ? null
          : WidgetStatePropertyAll(Size(.infinity, widget.menuHeight!)),
    );

    final labelChild = Text(buttonText, maxLines: 1, overflow: .ellipsis);
    Widget trigger = MenuAnchor(
      childFocusNode: _focus,
      style: menuStyle,
      onOpen: () {
        _syncAnchorWidth();
        if (mounted) setState(() {});
      },
      menuChildren: [
        for (final option in options)
          _ChoiceMenuItem<T>(
            option: option,
            selected: option.value == widget.selected,
            onPressed: widget.onSelected == null
                ? null
                : () {
                    if (option.value != widget.selected) {
                      widget.onSelected!(option.value);
                    }
                  },
          ),
      ],
      builder: (context, controller, child) => Semantics(
        label: widget.label,
        expanded: controller.isOpen,
        child: FilledButton(
          key: _anchorKey,
          focusNode: _focus,
          onPressed: widget.onSelected == null || options.isEmpty
              ? null
              : () {
                  if (controller.isOpen) {
                    controller.close();
                  } else {
                    _syncAnchorWidth();
                    setState(() {});
                    controller.open();
                  }
                },
          style: settingsSolidStyle(
            colors,
            background: colors.surfaceContainerHighest,
            disabledBackground: colors.onSurface.withValues(alpha: .12),
            foreground: colors.onSurface,
            height: settingsControlHeight,
            padding: const .fromLTRB(16, 0, 12, 0),
            radius: .circular(12),
          ),
          child: Row(
            mainAxisSize: stretch ? .max : .min,
            spacing: 6,
            children: [
              ?widget.leadingIcon,
              if (stretch)
                Expanded(child: labelChild)
              else
                Flexible(child: labelChild),
              Icon(
                controller.isOpen
                    ? Icons.keyboard_arrow_up_rounded
                    : Icons.keyboard_arrow_down_rounded,
                size: 20,
              ),
            ],
          ),
        ),
      ),
    );
    if (widget.width != null) {
      trigger = SizedBox(width: widget.width, child: trigger);
    } else if (!widget.fillWidth) {
      trigger = ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 280),
        child: trigger,
      );
    }
    return trigger;
  }
}

class _ChoiceMenuItem<T> extends StatefulWidget {
  const _ChoiceMenuItem({
    required this.option,
    required this.selected,
    required this.onPressed,
  });

  final SettingsChoiceOption<T> option;
  final bool selected;
  final VoidCallback? onPressed;

  @override
  State<_ChoiceMenuItem<T>> createState() => _ChoiceMenuItemState<T>();
}

class _ChoiceMenuItemState<T> extends State<_ChoiceMenuItem<T>> {
  late final _states = WidgetStatesController({if (widget.selected) .selected});

  @override
  void didUpdateWidget(_ChoiceMenuItem<T> oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.selected != widget.selected) {
      _states.update(.selected, widget.selected);
    }
  }

  @override
  void dispose() {
    _states.dispose();
    super.dispose();
  }

  @override
  // Inset, rounded items with a small gap, per MD3E expressive menus.
  Widget build(BuildContext context) => Padding(
    padding: const .symmetric(vertical: 1),
    child: MenuItemButton(
      autofocus: widget.selected,
      statesController: _states,
      leadingIcon: widget.option.leadingIcon,
      onPressed: widget.onPressed,
      style: MenuItemButton.styleFrom(
        minimumSize: const Size(0, settingsMenuItemHeight),
        padding: const .symmetric(horizontal: 16),
        shape: const RoundedRectangleBorder(borderRadius: .all(.circular(14))),
      ),
      child: Semantics(
        selected: widget.selected,
        child: Text(widget.option.label),
      ),
    ),
  );
}

/// Chooses between [SettingsSegmentedButton] (2–3 short options) and
/// [SettingsDropdown] (longer or more options).
class SettingsChoice<T> extends StatelessWidget {
  const SettingsChoice({
    super.key,
    required this.choices,
    required this.selected,
    required this.onSelected,
    this.label,
    this.hintText,
    this.width,
    this.menuHeight,
    this.fillWidth = false,
    this.maxSegmentLabelLength = 16,
  });

  final Map<T, String> choices;
  final T? selected;
  final ValueChanged<T>? onSelected;
  final String? label;
  final String? hintText;
  final double? width;
  final double? menuHeight;
  final bool fillWidth;
  final int maxSegmentLabelLength;

  static bool useSegments(Iterable<String> labels, {int maxLabelLength = 16}) =>
      labels.length >= 2 &&
      labels.length <= 3 &&
      labels.every((label) => label.length <= maxLabelLength);

  @override
  Widget build(BuildContext context) {
    if (useSegments(choices.values, maxLabelLength: maxSegmentLabelLength)) {
      return SettingsSegmentedButton<T>.choices(
        choices: choices,
        selected: selected,
        onSelected: onSelected,
        expand: fillWidth,
      );
    }
    return SettingsDropdown<T>(
      choices: choices,
      selected: selected,
      onSelected: onSelected,
      label: label,
      hintText: hintText,
      width: width,
      menuHeight: menuHeight,
      fillWidth: fillWidth,
    );
  }
}
