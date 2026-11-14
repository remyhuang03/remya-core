import 'package:material_ui/material_ui.dart';

/// The 44 px window title bar of Remya settings windows: icon, title and a
/// Close button. It does not move or close the window itself: [dragArea]
/// wraps the icon and title (for example window_manager's `DragToMoveArea`)
/// and [onClose] closes the window.
class RemyaTitleBar extends StatelessWidget {
  const RemyaTitleBar({
    super.key,
    required this.icon,
    required this.title,
    required this.onClose,
    this.dragArea,
  });

  /// Shown at 20 px, for example `Image.asset('assets/icons/app_icon.png')`.
  final Widget icon;
  final String title;
  final VoidCallback onClose;
  final Widget Function(Widget child)? dragArea;

  static const height = 44.0;

  @override
  Widget build(BuildContext context) {
    final label = Padding(
      padding: const .symmetric(horizontal: 12),
      child: Row(
        children: [
          SizedBox.square(dimension: 20, child: icon),
          const SizedBox(width: 10),
          Expanded(child: Text(title, overflow: .ellipsis)),
        ],
      ),
    );
    return Material(
      color: Theme.of(context).colorScheme.surface,
      child: SizedBox(
        height: height,
        child: Row(
          children: [
            Expanded(child: dragArea?.call(label) ?? label),
            IconButton(
              tooltip: 'Close',
              icon: const Icon(Icons.close_rounded, size: 18),
              onPressed: onClose,
            ),
            const SizedBox(width: 4),
          ],
        ),
      ),
    );
  }
}
