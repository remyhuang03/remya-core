import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:remya_client/settings_contract.dart';

/// Account state and avatar writes for Remya settings windows.
class RemyaAccountController extends ChangeNotifier {
  RemyaAccountController({required this.read, required this.save});

  factory RemyaAccountController.service({SettingsClient? client}) {
    final service = client ?? SettingsClient();
    final controller = RemyaAccountController(
      read: service.readAccountInformation,
      save: service.saveAccountAvatar,
    );
    if (client == null) controller._ownedClient = service;
    controller._changes = service.changes.listen((change) {
      if (change.topic == accountInformationKey) {
        if (controller.isBusy) {
          controller._refreshAfterSave = true;
        } else {
          unawaited(controller.reload());
        }
      }
    }, onError: (Object error) => debugPrint('Remya account: $error'));
    return controller;
  }

  final Future<Map<String, dynamic>> Function() read;
  final Future<Map<String, dynamic>> Function(String, Map<String, double>) save;
  Map<String, dynamic> value = {};
  String? errorMessage;
  bool isBusy = false;
  SettingsClient? _ownedClient;
  StreamSubscription<SettingsChange>? _changes;
  bool _disposed = false;
  bool _refreshAfterSave = false;
  int _revision = 0;

  Future<void> reload() async {
    if (_disposed || isBusy) return;
    final revision = ++_revision;
    try {
      final next = await read();
      if (_disposed || revision != _revision) return;
      value = next;
      errorMessage = next['error'] as String?;
    } catch (error) {
      if (_disposed || revision != _revision) return;
      errorMessage = '$error';
    }
    notifyListeners();
  }

  Future<void> saveAvatar(String path, Map<String, double> crop) async {
    if (_disposed || isBusy) return;
    _revision++;
    isBusy = true;
    notifyListeners();
    try {
      final next = await save(path, crop);
      if (_disposed) return;
      value = next;
      errorMessage = next['error'] as String?;
    } catch (error) {
      if (_disposed) return;
      errorMessage = '$error';
    } finally {
      isBusy = false;
      if (!_disposed) {
        notifyListeners();
        if (_refreshAfterSave) {
          _refreshAfterSave = false;
          unawaited(reload());
        }
      }
    }
  }

  @override
  void dispose() {
    _disposed = true;
    final cancel = _changes?.cancel();
    _changes = null;
    final owned = _ownedClient;
    _ownedClient = null;
    if (owned != null) {
      unawaited(
        (cancel ?? Future<void>.value())
            .catchError((Object _) {})
            .whenComplete(() => owned.close().catchError((Object _) {})),
      );
    } else if (cancel != null) {
      unawaited(cancel.catchError((Object _) {}));
    }
    super.dispose();
  }
}
