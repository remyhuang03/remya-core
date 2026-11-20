import 'dart:convert';

import 'dbus_client.dart';

const googleAccountTopic = 'google-account';

class GoogleAccountStatus {
  const GoogleAccountStatus({
    required this.configured,
    required this.pending,
    required this.email,
    required this.calendar,
    required this.tasks,
    required this.error,
  });

  factory GoogleAccountStatus.fromJson(Map<String, dynamic> json) =>
      GoogleAccountStatus(
        configured: json['configured'] as bool,
        pending: json['pending'] as bool,
        email: json['email'] as String?,
        calendar: json['calendar'] as bool,
        tasks: json['tasks'] as bool,
        error: json['error'] as String?,
      );

  final bool configured;
  final bool pending;
  final String? email;
  final bool calendar;
  final bool tasks;
  final String? error;
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

  Future<GoogleAccountStatus> disconnectGoogleAccount() async =>
      GoogleAccountStatus.fromJson(
        await call('Set', ['$googleAccountTopic.disconnect', '{}']),
      );
}
