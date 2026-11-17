import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:dbus/dbus.dart';
import 'package:fake_async/fake_async.dart';
import 'package:remya_client/remya_client.dart';
import 'package:test/test.dart';

class _MethodCallRecord {
  _MethodCallRecord({
    required this.destination,
    required this.path,
    required this.interface,
    required this.name,
    required this.values,
    required this.replySignature,
  });

  final String? destination;
  final DBusObjectPath path;
  final String? interface;
  final String name;
  final List<DBusValue> values;
  final DBusSignature? replySignature;
}

class _FakeDBusClient extends DBusClient {
  _FakeDBusClient() : super(DBusAddress('unix:path=/dev/null'));

  final calls = <_MethodCallRecord>[];
  int closeCount = 0;
  Future<DBusMethodSuccessResponse> Function(_MethodCallRecord record)?
  onCallMethod;

  @override
  Future<DBusMethodSuccessResponse> callMethod({
    String? destination,
    required DBusObjectPath path,
    String? interface,
    required String name,
    Iterable<DBusValue> values = const [],
    DBusSignature? replySignature,
    bool noReplyExpected = false,
    bool noAutoStart = false,
    bool allowInteractiveAuthorization = false,
  }) {
    final record = _MethodCallRecord(
      destination: destination,
      path: path,
      interface: interface,
      name: name,
      values: values.toList(),
      replySignature: replySignature,
    );
    calls.add(record);
    final handler = onCallMethod;
    if (handler != null) {
      return handler(record);
    }
    return Future.value(DBusMethodSuccessResponse([DBusString('{}')]));
  }

  @override
  Future<void> close() async {
    closeCount++;
  }
}

class _CountingDBusClient extends DBusClient {
  _CountingDBusClient(super.address);

  int closeCount = 0;

  @override
  Future<void> close() async {
    closeCount++;
    await super.close();
  }
}

class _TestCoreObject extends DBusObject {
  _TestCoreObject() : super(DBusObjectPath(settingsObjectPath));

  final handledCalls = <(String method, List<String> args)>[];
  Map<String, dynamic> themeValue = {
    'version': 1,
    'theme': {'theme_mode': 'light'},
  };
  String Function(String method, List<String> args)? customReply;
  DBusMethodErrorResponse? errorReply;

  @override
  Future<DBusMethodResponse> handleMethodCall(DBusMethodCall methodCall) async {
    if (methodCall.interface != settingsInterface) {
      return DBusMethodErrorResponse.unknownInterface();
    }
    final args = methodCall.values.map((value) => value.asString()).toList();
    handledCalls.add((methodCall.name, args));
    if (errorReply case final error?) {
      return error;
    }
    if (customReply case final reply?) {
      return DBusMethodSuccessResponse([
        DBusString(reply(methodCall.name, args)),
      ]);
    }
    if (methodCall.name == 'Get' && args.singleOrNull == 'theme') {
      return DBusMethodSuccessResponse([DBusString(jsonEncode(themeValue))]);
    }
    return DBusMethodSuccessResponse([
      DBusString(jsonEncode({'method': methodCall.name, 'args': args})),
    ]);
  }
}

Future<
  ({
    DBusServer server,
    DBusAddress address,
    DBusClient serviceBus,
    _TestCoreObject object,
    Directory tempDir,
  })
>
_startTestBus() async {
  final tempDir = await Directory.systemTemp.createTemp('remya_client_test_');
  final server = DBusServer();
  final address = await server.listenAddress(DBusAddress.unix(dir: tempDir));
  final serviceBus = DBusClient(address);
  final object = _TestCoreObject();
  await serviceBus.requestName(settingsBusName);
  await serviceBus.registerObject(object);
  return (
    server: server,
    address: address,
    serviceBus: serviceBus,
    object: object,
    tempDir: tempDir,
  );
}

void main() {
  group('SettingsClient method mapping and JSON handling', () {
    test('rejects unknown methods and mismatched argument arities', () async {
      final bus = _FakeDBusClient();
      final client = SettingsClient(bus: bus);

      await expectLater(client.call('DoesNotExist'), throwsArgumentError);
      await expectLater(client.call('Get'), throwsArgumentError);
      await expectLater(client.call('Get', ['a', 'b']), throwsArgumentError);
      await expectLater(client.call('Set', ['only-one']), throwsArgumentError);
      await expectLater(
        client.call('Replace', ['id', 'value']),
        throwsArgumentError,
      );
      await expectLater(client.call('Reload', ['extra']), throwsArgumentError);
      expect(bus.calls, isEmpty);
    });

    test(
      'maps every Core1 method to DBus arguments and decodes JSON',
      () async {
        final bus = _FakeDBusClient();
        final client = SettingsClient(bus: bus);
        bus.onCallMethod = (record) async {
          final args = record.values.map((v) => v.asString()).toList();
          return DBusMethodSuccessResponse([
            DBusString(
              jsonEncode({'ok': true, 'method': record.name, 'args': args}),
            ),
          ]);
        };

        final invocations = <(String, List<String>)>[
          ('Get', ['theme']),
          ('Manage', ['display']),
          ('Set', ['themes.material-theme-mode', 'dark']),
          ('Replace', ['remya-bar.pos', '"top"', 'rev1']),
          ('OpenApp', ['remya-bar', 'general']),
          ('List', ['']),
          ('Search', ['accent']),
          ('Open', ['home']),
          ('Reload', []),
          ('Resolve', ['display', 'keep']),
          ('Quit', []),
          ('Close', []),
          ('Restart', []),
        ];

        expect(invocations.length, settingsMethods.length);
        for (final (method, args) in invocations) {
          final result = await client.call(method, args);
          expect(result, {'ok': true, 'method': method, 'args': args});
        }

        expect(bus.calls.length, settingsMethods.length);
        for (var i = 0; i < invocations.length; i++) {
          final (method, args) = invocations[i];
          final record = bus.calls[i];
          expect(record.destination, settingsBusName);
          expect(record.path, DBusObjectPath(settingsObjectPath));
          expect(record.interface, settingsInterface);
          expect(record.name, method);
          expect(record.replySignature, DBusSignature('s'));
          expect(record.values, args.map((arg) => DBusString(arg)).toList());
        }

        final managed = await client.manage('input');
        expect(managed, {
          'ok': true,
          'method': 'Manage',
          'args': ['input'],
        });
      },
    );

    test('surfaces malformed JSON, non-object JSON, and DBus errors', () async {
      final bus = _FakeDBusClient();
      final client = SettingsClient(bus: bus);

      bus.onCallMethod = (_) async =>
          DBusMethodSuccessResponse([DBusString('not-json{')]);
      await expectLater(client.call('Get', ['theme']), throwsFormatException);

      bus.onCallMethod = (_) async =>
          DBusMethodSuccessResponse([DBusString('[1, 2, 3]')]);
      await expectLater(
        client.call('Get', ['theme']),
        throwsA(isA<TypeError>()),
      );

      bus.onCallMethod = (_) async =>
          DBusMethodSuccessResponse([DBusString('null')]);
      await expectLater(
        client.call('Get', ['theme']),
        throwsA(isA<TypeError>()),
      );

      bus.onCallMethod = (_) async => throw DBusMethodResponseException(
        DBusMethodErrorResponse('top.remya.Core1.OperationFailed', [
          DBusString('Changed in another view.'),
        ]),
      );
      await expectLater(
        client.call('Get', ['theme']),
        throwsA(isA<DBusMethodResponseException>()),
      );
    });

    test('applies 30s default timeout and 300s package-upgrade timeout', () {
      fakeAsync((async) {
        final bus = _FakeDBusClient();
        bus.onCallMethod = (_) => Completer<DBusMethodSuccessResponse>().future;
        final client = SettingsClient(bus: bus);

        Object? standardError;
        unawaited(
          client
              .call('Get', ['theme'])
              .then<void>(
                (_) {},
                onError: (Object error) {
                  standardError = error;
                },
              ),
        );
        async.elapse(const Duration(seconds: 29));
        expect(standardError, isNull);
        async.elapse(const Duration(seconds: 1));
        expect(standardError, isA<TimeoutException>());

        Object? upgradeError;
        unawaited(
          client
              .call('Set', ['installed.c3c4a29c', 'true'])
              .then<void>(
                (_) {},
                onError: (Object error) {
                  upgradeError = error;
                },
              ),
        );
        async.elapse(const Duration(seconds: 299));
        expect(upgradeError, isNull);
        async.elapse(const Duration(seconds: 1));
        expect(upgradeError, isA<TimeoutException>());
      });
    });
  });

  group('SettingsClient connection ownership and watch()', () {
    test('rejects ownsBus: false when bus is internally created', () {
      expect(() => SettingsClient(ownsBus: false), throwsArgumentError);
    });

    test(
      'injected bus stays open when one of two shared clients closes',
      () async {
        final harness = await _startTestBus();
        final sharedBus = _CountingDBusClient(harness.address);
        addTearDown(() async {
          await sharedBus.close();
          await harness.serviceBus.close();
          await harness.server.close();
          await harness.tempDir.delete(recursive: true);
        });

        final client1 = SettingsClient(bus: sharedBus);
        final client2 = SettingsClient(bus: sharedBus);

        expect((await client1.call('Get', ['theme']))['version'], 1);
        expect((await client2.call('Get', ['theme']))['version'], 1);

        final received = <SettingsChange>[];
        final sub = client2.changes.listen(received.add);
        addTearDown(sub.cancel);
        await Future<void>.delayed(const Duration(milliseconds: 50));

        await client1.close();
        expect(sharedBus.closeCount, 0);

        // client2 continues issuing RPCs and receiving signals after client1 closes.
        harness.object.themeValue = {
          'version': 1,
          'theme': {'theme_mode': 'dark'},
        };
        expect(
          (await client2.call('Get', ['theme']))['theme']['theme_mode'],
          'dark',
        );
        await harness.object.emitSignal(settingsInterface, 'Changed', [
          DBusString('theme'),
          DBusString(jsonEncode(harness.object.themeValue)),
        ]);
        await Future<void>.delayed(const Duration(milliseconds: 50));
        expect(received, hasLength(1));
        expect(received.single.topic, 'theme');

        await client2.close();
        expect(sharedBus.closeCount, 0);
      },
    );

    test('explicit ownsBus: true closes the bus at most once', () async {
      final bus = _FakeDBusClient();
      final client = SettingsClient(bus: bus, ownsBus: true);

      await client.close();
      await client.close();

      expect(bus.closeCount, 1);
    });

    test('default SettingsClient closes cleanly and idempotently', () async {
      final client = SettingsClient();
      await client.close();
      await client.close();
    });

    test('watch() yields initial theme, rereads theme signals, passes other topics, and cancels cleanly', () async {
      final harness = await _startTestBus();
      final bus = _CountingDBusClient(harness.address);
      addTearDown(() async {
        await bus.close();
        await harness.serviceBus.close();
        await harness.server.close();
        await harness.tempDir.delete(recursive: true);
      });

      final client = SettingsClient(bus: bus, ownsBus: true);
      final events = <SettingsChange>[];
      final errors = <Object>[];
      var done = false;

      final sub = client.watch().listen(
        events.add,
        onError: errors.add,
        onDone: () => done = true,
      );

      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(events, hasLength(1));
      expect(events.first.topic, 'theme');
      expect(events.first.value['theme']['theme_mode'], 'light');

      // Update server state before emitting signal with stale payload; watch() must reread Get('theme').
      harness.object.themeValue = {
        'version': 1,
        'theme': {'theme_mode': 'dark'},
      };
      await harness.object.emitSignal(settingsInterface, 'Changed', [
        DBusString('theme'),
        DBusString(
          jsonEncode({
            'version': 1,
            'theme': {'theme_mode': 'stale'},
          }),
        ),
      ]);
      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(events, hasLength(2));
      expect(events[1].topic, 'theme');
      expect(events[1].value['theme']['theme_mode'], 'dark');

      // Non-theme topics pass through directly without an extra Get call.
      final callsBeforeAccent = harness.object.handledCalls.length;
      await harness.object.emitSignal(settingsInterface, 'Changed', [
        DBusString('accent'),
        DBusString(jsonEncode({'color': '#112233'})),
      ]);
      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(events, hasLength(3));
      expect(events[2].topic, 'accent');
      expect(events[2].value, {'color': '#112233'});
      expect(harness.object.handledCalls.length, callsBeforeAccent);

      // Cancel watch() and verify subsequent signals do not arrive.
      await sub.cancel();
      await harness.object.emitSignal(settingsInterface, 'Changed', [
        DBusString('accent'),
        DBusString(jsonEncode({'color': '#445566'})),
      ]);
      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(events, hasLength(3));
      expect(errors, isEmpty);
      expect(done, isFalse);

      // Client close cancels any remaining watchers before closing owned bus.
      final events2 = <SettingsChange>[];
      var done2 = false;
      final sub2 = client.watch().listen(
        events2.add,
        onDone: () => done2 = true,
      );
      await Future<void>.delayed(const Duration(milliseconds: 50));
      expect(events2, hasLength(1));

      await client.close();
      await Future<void>.delayed(const Duration(milliseconds: 20));
      expect(done2, isTrue);
      expect(bus.closeCount, 1);
      await sub2.cancel();
    });

    test('changes stream surfaces malformed signal JSON as error', () async {
      final harness = await _startTestBus();
      final bus = DBusClient(harness.address);
      addTearDown(() async {
        await bus.close();
        await harness.serviceBus.close();
        await harness.server.close();
        await harness.tempDir.delete(recursive: true);
      });

      final client = SettingsClient(bus: bus);
      final errors = <Object>[];
      final sub = client.changes.listen((_) {}, onError: errors.add);
      addTearDown(sub.cancel);

      await Future<void>.delayed(const Duration(milliseconds: 50));
      await harness.object.emitSignal(settingsInterface, 'Changed', [
        DBusString('theme'),
        DBusString('{bad json'),
      ]);
      await Future<void>.delayed(const Duration(milliseconds: 50));

      expect(errors.single, isA<FormatException>());
    });
  });
}
