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

class GoogleCalendarException implements Exception {
  const GoogleCalendarException(this.code, this.message);

  final String code;
  final String message;

  @override
  String toString() => message;
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

  /// Begin interactive authorization for the requested Google capabilities.
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

  Future<GoogleAccountStatus> cancelGoogleAuthorization() async =>
      GoogleAccountStatus.fromJson(
        await call('Set', ['$googleAccountTopic.cancel', '{}']),
      );

  Future<GoogleAccountStatus> disconnectGoogleAccount() async =>
      GoogleAccountStatus.fromJson(
        await call('Set', ['$googleAccountTopic.disconnect', '{}']),
      );

  Future<GoogleAccountStatus> connectGoogleCalendar() =>
      connectGoogleAccount(calendar: true);

  /// Read calendars and recurring event instances for the expected identity.
  Future<Map<String, dynamic>> googleCalendarRequest({
    required String accountId,
    required String timeMin,
    required String timeMax,
    List<String>? calendarIds,
    bool cacheOnly = false,
  }) async {
    const retry = GoogleCalendarException(
      'retry',
      'Google Calendar did not respond.',
    );
    try {
      return await call('GoogleCalendar', [
        jsonEncode({
          'accountId': accountId,
          'timeMin': timeMin,
          'timeMax': timeMax,
          'calendarIds': ?calendarIds,
          if (cacheOnly) 'cacheOnly': true,
        }),
      ]);
    } on TimeoutException {
      throw retry;
    } on DBusMethodResponseException catch (error) {
      final code = switch (error.errorName) {
        '$settingsInterface.CalendarAuth' => 'auth',
        '$settingsInterface.CalendarOffline' => 'offline',
        '$settingsInterface.CalendarRetry' => 'retry',
        '$settingsInterface.InvalidArgument' => 'invalid',
        '$settingsInterface.Unavailable' => 'unavailable',
        'org.freedesktop.DBus.Error.UnknownMethod' =>
          throw const GoogleCalendarException(
            'unavailable',
            'Remya Core does not support Google Calendar.',
          ),
        'org.freedesktop.DBus.Error.ServiceUnknown' ||
        'org.freedesktop.DBus.Error.NameHasNoOwner' =>
          throw const GoogleCalendarException(
            'unavailable',
            'Remya Core is unavailable.',
          ),
        'org.freedesktop.DBus.Error.NoReply' ||
        'org.freedesktop.DBus.Error.Disconnected' => throw retry,
        _ => 'retry',
      };
      final value = error.response.values.firstOrNull;
      throw GoogleCalendarException(
        code,
        value is DBusString ? value.value : 'Could not reach Google Calendar.',
      );
    }
  }

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
