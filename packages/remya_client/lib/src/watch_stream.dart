import 'dart:async';

import 'dbus_client.dart';

/// Bridges a broadcast change stream into an initial read followed by mapped updates
/// without missing concurrent changes or deadlocking on subscription cancellation.
Stream<T> watchChangesStream<T>({
  required Stream<SettingsChange> source,
  required Future<T> Function() initial,
  required Future<T> Function(SettingsChange change) onChange,
  Set<Future<void> Function()>? closeHooks,
  bool Function()? isClosed,
}) {
  late final StreamController<T> controller;
  StreamSubscription<SettingsChange>? subscription;
  StreamController<SettingsChange>? pending;
  Future<void>? stopping;
  var canceled = false;

  Future<void> stop({bool closeController = true}) {
    return stopping ??= () async {
      canceled = true;
      closeHooks?.remove(stop);
      await subscription?.cancel();
      subscription = null;
      final buffer = pending;
      pending = null;
      if (buffer != null && !buffer.isClosed) {
        unawaited(buffer.close());
      }
      if (closeController && !controller.isClosed) {
        unawaited(controller.close());
      }
    }();
  }

  controller = StreamController<T>(
    onListen: () {
      if (isClosed?.call() ?? false) {
        unawaited(controller.close());
        return;
      }
      final buffer = pending = StreamController<SettingsChange>();
      subscription = source.listen(
        buffer.add,
        onError: buffer.addError,
        onDone: buffer.close,
      );
      closeHooks?.add(stop);

      unawaited(() async {
        try {
          final first = await initial();
          if (canceled || (isClosed?.call() ?? false)) return;
          controller.add(first);
          await for (final change in buffer.stream) {
            if (canceled || (isClosed?.call() ?? false)) break;
            final next = await onChange(change);
            if (canceled || (isClosed?.call() ?? false)) break;
            controller.add(next);
          }
        } catch (error, stackTrace) {
          if (!canceled && !controller.isClosed) {
            controller.addError(error, stackTrace);
          }
        } finally {
          await stop();
        }
      }());
    },
    onCancel: () => stop(closeController: false),
  );

  return controller.stream;
}
