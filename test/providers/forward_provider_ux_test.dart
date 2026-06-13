import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/forward_config.dart';
import 'package:tunnel_pilot/models/forward_status.dart';
import 'package:tunnel_pilot/providers/forward_provider.dart';
import 'package:tunnel_pilot/services/log_service.dart';
import 'package:tunnel_pilot/services/notification_service.dart';
import 'package:tunnel_pilot/services/ssh_tunnel_service.dart';
import 'package:tunnel_pilot/services/storage_service.dart';

// ── Mocks ──────────────────────────────────────────────────────────────────

class MockStorageService extends StorageService {
  @override
  Future<void> saveForwards(List<ForwardConfig> forwards) async {}
}

class MockNotificationService extends NotificationService {
  final List<String> connected = [];
  final List<String> disconnected = [];
  final List<String> errors = [];

  @override
  Future<void> showConnected(String name) async => connected.add(name);

  @override
  Future<void> showDisconnected(String name) async => disconnected.add(name);

  @override
  Future<void> showError(String name, String error) async => errors.add(name);
}

/// A SshTunnelService where each connect() call resolves with a controllable outcome.
/// By default, connect() fires [connecting → error] immediately (simulates failed SSH).
/// Set [nextConnectStatus] to override the terminal status for the next connect call.
class ControllableTunnelService extends SshTunnelService {
  ForwardStatus nextConnectStatus = ForwardStatus.error;
  String? nextConnectError = 'mock error';

  /// Completer used to delay the connect resolution until the test is ready.
  Completer<void>? _connectHold;

  /// Hold the next connect until [releaseConnect] is called.
  void holdNextConnect() {
    _connectHold = Completer<void>();
  }

  void releaseConnect() {
    _connectHold?.complete();
    _connectHold = null;
  }

  @override
  Future<void> connect(
    ForwardConfig config, {
    required StatusCallback onStatusChanged,
  }) async {
    onStatusChanged(config.id, ForwardStatus.connecting, null);
    if (_connectHold != null) await _connectHold!.future;
    onStatusChanged(config.id, nextConnectStatus, nextConnectError);
  }

  @override
  Future<void> disconnect(String id) async {}
}

// ── Helpers ────────────────────────────────────────────────────────────────

ForwardConfig makeConfig({String id = 'tid', int localPort = 9000}) {
  return ForwardConfig(
    id: id,
    name: 'Test',
    sshHost: 'host',
    sshUsername: 'user',
    sshPassword: 'pass',
    localPort: localPort,
    remoteHost: 'remote',
    remotePort: 80,
  );
}

ForwardProvider makeProvider({
  ControllableTunnelService? tunnel,
  MockNotificationService? notification,
  bool autoReconnect = false,
  int maxRetries = 3,
  int delaySec = 0,
  bool skipPortWait = true,
}) {
  return ForwardProvider(
    storage: MockStorageService(),
    tunnel: tunnel ?? ControllableTunnelService(),
    notification: notification ?? MockNotificationService(),
    logService: LogService(),
    autoReconnect: autoReconnect,
    autoReconnectDelaySec: delaySec,
    autoReconnectMaxRetries: maxRetries,
  )..skipPortWait = skipPortWait;
}

// ── Tests ──────────────────────────────────────────────────────────────────

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  // ── Bug 3: Error status in tray should reconnect, not disconnect ──────────
  group('toggleForward on error status', () {
    test('reconnects instead of disconnecting', () async {
      final tunnel = ControllableTunnelService();
      final provider = makeProvider(tunnel: tunnel);
      final config = makeConfig();
      await provider.addForward(config);

      // Put tunnel into error state directly
      provider.forceStatus(config.id, ForwardStatus.error);

      // Now toggle — should reconnect
      await provider.toggleForward(config.id);

      // Status should be connecting (or transitioned beyond, not disconnected)
      expect(
        provider.getStatus(config.id),
        isNot(ForwardStatus.disconnected),
        reason: 'error toggle should reconnect, not stay disconnected',
      );
    });

    test('does not add to _userDisconnected on error toggle', () async {
      final tunnel = ControllableTunnelService();
      final provider = makeProvider(tunnel: tunnel, autoReconnect: true, delaySec: 0);
      final config = makeConfig();
      await provider.addForward(config);

      provider.forceStatus(config.id, ForwardStatus.error);

      await provider.toggleForward(config.id);

      // If _userDisconnected were set, auto-reconnect would be blocked.
      // We verify by checking that connect was called (status is connecting or error from mock).
      expect(
        provider.getStatus(config.id),
        anyOf(ForwardStatus.connecting, ForwardStatus.error),
        reason: 'should have attempted reconnect, not blocked by _userDisconnected',
      );
    });

    test('connected toggle still disconnects', () async {
      final tunnel = ControllableTunnelService();
      final provider = makeProvider(tunnel: tunnel);
      final config = makeConfig();
      await provider.addForward(config);

      provider.forceStatus(config.id, ForwardStatus.connected);

      await provider.toggleForward(config.id);

      expect(provider.getStatus(config.id), ForwardStatus.disconnected);
    });

    test('sets connecting status immediately on error toggle', () async {
      final tunnel = ControllableTunnelService();
      tunnel.holdNextConnect();
      final provider = makeProvider(tunnel: tunnel);
      final config = makeConfig();
      await provider.addForward(config);

      provider.forceStatus(config.id, ForwardStatus.error);

      final toggleFuture = provider.toggleForward(config.id);

      // Before connect resolves, status must be connecting
      expect(provider.getStatus(config.id), ForwardStatus.connecting);

      tunnel.releaseConnect();
      await toggleFuture;
    });
  });

  // ── Bug 2: Reconnect smooth — connecting shown immediately ────────────────
  group('auto-reconnect UX', () {
    test('shows connecting status immediately when reconnect timer fires', () async {
      final tunnel = ControllableTunnelService();
      final provider = makeProvider(
        tunnel: tunnel,
        autoReconnect: true,
        maxRetries: 1,
      );
      final config = makeConfig();
      await provider.addForward(config);

      final statuses = <ForwardStatus>[];
      provider.addListener(() => statuses.add(provider.getStatus(config.id)));

      // fireReconnectNow executes the timer body synchronously — status must
      // be set to connecting BEFORE _connectForward is awaited.
      await provider.fireReconnectNow(config.id);

      expect(
        statuses.contains(ForwardStatus.connecting),
        isTrue,
        reason: 'connecting status should appear immediately when timer fires',
      );
    });

    test('reconnect fires _connectForward after setting connecting', () async {
      final tunnel = ControllableTunnelService();
      tunnel.nextConnectStatus = ForwardStatus.connected;
      tunnel.nextConnectError = null;
      final provider = makeProvider(tunnel: tunnel, autoReconnect: true);
      final config = makeConfig();
      await provider.addForward(config);

      provider.forceStatus(config.id, ForwardStatus.error);
      await provider.fireReconnectNow(config.id);

      // After reconnect completes with success, status should be connected
      expect(provider.getStatus(config.id), ForwardStatus.connected);
    });
  });

  // ── Bug 4: Notification spam ───────────────────────────────────────────────
  group('notification behavior', () {
    test('user-initiated disconnect fires NO disconnect notification', () async {
      final tunnel = ControllableTunnelService();
      final notif = MockNotificationService();
      final provider = makeProvider(tunnel: tunnel, notification: notif);
      final config = makeConfig();
      await provider.addForward(config);

      // Put into connected state
      provider.forceStatus(config.id, ForwardStatus.connected);

      // User toggles disconnect
      await provider.toggleForward(config.id);

      expect(
        notif.disconnected,
        isEmpty,
        reason: 'user-initiated disconnect should not fire notification',
      );
    });

    test('error with retries remaining fires NO error notification', () async {
      final tunnel = ControllableTunnelService();
      final notif = MockNotificationService();
      final provider = makeProvider(
        tunnel: tunnel,
        notification: notif,
        autoReconnect: true,
        maxRetries: 3,
        delaySec: 60, // long delay so timer doesn't fire during test
      );
      final config = makeConfig();
      await provider.addForward(config);

      // Toggle to trigger first connect which will error
      await provider.toggleForward(config.id);

      // Attempt 1 out of 3 — retries remain — should be silent
      expect(
        notif.errors,
        isEmpty,
        reason: 'should not notify error when retries remain',
      );
    });

    test('error with NO retries remaining fires error notification', () async {
      final tunnel = ControllableTunnelService();
      final notif = MockNotificationService();
      final provider = makeProvider(
        tunnel: tunnel,
        notification: notif,
        autoReconnect: true,
        maxRetries: 1,
      );
      final config = makeConfig();
      await provider.addForward(config);

      // 1st connect → error → attempt 0 < 1 → schedules timer (silent)
      await provider.toggleForward(config.id);
      expect(notif.errors, isEmpty, reason: 'should still be silent on 1st error');

      // Simulate timer firing → 2nd connect → error → attempt 1 >= 1 → notification
      await provider.fireReconnectNow(config.id);

      expect(
        notif.errors,
        isNotEmpty,
        reason: 'should notify when all retries exhausted',
      );
    });

    test('error with autoReconnect disabled fires error notification immediately', () async {
      final tunnel = ControllableTunnelService();
      final notif = MockNotificationService();
      final provider = makeProvider(
        tunnel: tunnel,
        notification: notif,
        autoReconnect: false,
      );
      final config = makeConfig();
      await provider.addForward(config);

      await provider.toggleForward(config.id);

      expect(
        notif.errors,
        isNotEmpty,
        reason: 'no auto-reconnect means error is final, must notify immediately',
      );
    });

    test('connect success always notifies', () async {
      final tunnel = ControllableTunnelService()
        ..nextConnectStatus = ForwardStatus.connected
        ..nextConnectError = null;
      final notif = MockNotificationService();
      final provider = makeProvider(tunnel: tunnel, notification: notif);
      final config = makeConfig();
      await provider.addForward(config);

      await provider.toggleForward(config.id);

      expect(notif.connected, contains(config.name));
    });

    test('disconnectAll fires no notifications', () async {
      final tunnel = ControllableTunnelService();
      final notif = MockNotificationService();
      final provider = makeProvider(tunnel: tunnel, notification: notif);
      final c1 = makeConfig(id: 'a', localPort: 9001);
      final c2 = makeConfig(id: 'b', localPort: 9002);
      await provider.addForward(c1);
      await provider.addForward(c2);

      provider.forceStatus('a', ForwardStatus.connected);
      provider.forceStatus('b', ForwardStatus.connected);

      await provider.disconnectAll();

      expect(notif.disconnected, isEmpty,
          reason: 'disconnectAll should not spam notifications');
    });
  });
}
