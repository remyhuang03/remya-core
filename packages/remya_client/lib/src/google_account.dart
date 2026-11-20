import 'dart:async';
import 'dart:convert';

import 'package:dbus/dbus.dart';

import 'dbus_client.dart';

const googleAccountTopic = 'google-account';

class GoogleAccountStatus {
  const GoogleAccountStatus({
    required this.configured,
    required this.pending,
    required this.email,
    this.accountId,
    required this.calendar,
    required this.tasks,
    this.avatarSync = false,
    required this.error,
  });

  factory GoogleAccountStatus.fromJson(Map<String, dynamic> json) =>
      GoogleAccountStatus(
        configured: json['configured'] as bool,
        pending: json['pending'] as bool,
        email: json['email'] as String?,
        accountId: json['accountId'] as String?,
        calendar: json['calendar'] as bool,
        tasks: json['tasks'] as bool,
        avatarSync: json['avatarSync'] as bool? ?? false,
        error: json['error'] as String?,
      );

  final bool configured;
  final bool pending;
  final String? email;
  final String? accountId;
  final bool calendar;
  final bool tasks;
  final bool avatarSync;
  final String? error;
}

class GoogleTasksException implements Exception {
  const GoogleTasksException(this.code, this.message);

  final String code;
  final String message;

  @override
  String toString() => message;
}

extension GoogleAccountClient on SettingsClient {
  Future<GoogleAccountStatus> googleAccountStatus() async =>
      GoogleAccountStatus.fromJson(await call('Get', [googleAccountTopic]));

  /// Begin interactive authorization for the requested read/write capabilities.
  Future<GoogleAccountStatus> connectGoogleAccount({
    bool calendar = false,
    bool tasks = false,
  }) async => GoogleAccountStatus.fromJson(
    await call('Set', [
      '$googleAccountTopic.connect',
      jsonEncode({'calendar': calendar, 'tasks': tasks}),
    ]),
  );

  /// Enables shared avatar sync, requesting Google profile access if needed.
  Future<GoogleAccountStatus> setGoogleAvatarSync(bool enabled) async =>
      GoogleAccountStatus.fromJson(
        await call('Set', [
          '$googleAccountTopic.avatar-sync',
          jsonEncode(enabled),
        ]),
      );

  Future<GoogleAccountStatus> disconnectGoogleAccount() async =>
      GoogleAccountStatus.fromJson(
        await call('Set', ['$googleAccountTopic.disconnect', '{}']),
      );

  Future<GoogleAccountStatus> connectGoogleTasks() =>
      connectGoogleAccount(tasks: true);

  /// Call an allowlisted Tasks v1 operation for the expected Google identity.
  Future<Map<String, dynamic>> googleTasksRequest(
    String accountId,
    String operation, {
    Map<String, dynamic> args = const {},
  }) async {
    try {
      return await call('GoogleTasks', [
        jsonEncode({
          'accountId': accountId,
          'operation': operation,
          'args': args,
        }),
      ]);
    } on TimeoutException {
      throw _tasksConnectionFailure(operation);
    } on DBusMethodResponseException catch (error) {
      final code = switch (error.errorName) {
        '$settingsInterface.TasksAuth' => 'auth',
        '$settingsInterface.TasksOffline' => 'offline',
        '$settingsInterface.TasksRetry' => 'retry',
        '$settingsInterface.TasksMissing' => 'missing',
        '$settingsInterface.TasksDenied' => 'denied',
        '$settingsInterface.TasksAmbiguous' => 'ambiguous',
        '$settingsInterface.InvalidArgument' => 'invalid',
        '$settingsInterface.Unavailable' => 'unavailable',
        '$settingsInterface.OperationFailed' => 'error',
        'org.freedesktop.DBus.Error.UnknownMethod' =>
          throw const GoogleTasksException(
            'unavailable',
            'Remya Core does not support Google Tasks.',
          ),
        'org.freedesktop.DBus.Error.ServiceUnknown' ||
        'org.freedesktop.DBus.Error.NameHasNoOwner' =>
          throw const GoogleTasksException(
            'unavailable',
            'Remya Core is unavailable.',
          ),
        'org.freedesktop.DBus.Error.NoReply' ||
        'org.freedesktop.DBus.Error.Disconnected' =>
          throw _tasksConnectionFailure(operation),
        _ => 'error',
      };
      final value = error.response.values.firstOrNull;
      throw GoogleTasksException(
        code,
        value is DBusString ? value.value : 'Could not reach Google Tasks.',
      );
    }
  }
}

GoogleTasksException _tasksConnectionFailure(
  String operation,
) => switch (operation) {
  'lists.insert' || 'tasks.insert' => const GoogleTasksException(
    'ambiguous',
    'Google may have created this item. Check the list before adding it again.',
  ),
  'tasks.move' || 'tasks.clear' => const GoogleTasksException(
    'ambiguous',
    'Google may have applied this change. Check the list before trying again.',
  ),
  _ => const GoogleTasksException('retry', 'Google Tasks did not respond.'),
};
