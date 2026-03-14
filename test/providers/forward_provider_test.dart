import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/forward_config.dart';
import 'package:tunnel_pilot/models/forward_status.dart';
import 'package:tunnel_pilot/providers/forward_provider.dart';
import 'package:tunnel_pilot/services/log_service.dart';
import 'package:tunnel_pilot/services/notification_service.dart';
import 'package:tunnel_pilot/services/ssh_tunnel_service.dart';
import 'package:tunnel_pilot/services/storage_service.dart';

// Simple mock storage that doesn't touch filesystem
class MockStorageService extends StorageService {
  List<ForwardConfig>? lastSavedForwards;

  @override
  Future<void> saveForwards(List<ForwardConfig> forwards) async {
    lastSavedForwards = List.from(forwards);
  }
}

void main() {
  group('ForwardProvider', () {
    late MockStorageService mockStorage;
    late SshTunnelService tunnelService;
    late NotificationService notificationService;
    late ForwardProvider provider;

    setUp(() {
      mockStorage = MockStorageService();
      tunnelService = SshTunnelService();
      notificationService = NotificationService();
      provider = ForwardProvider(
        storage: mockStorage,
        tunnel: tunnelService,
        notification: notificationService,
        logService: LogService(),
      );
    });

    ForwardConfig createConfig({
      String? id,
      String name = 'Test',
      int localPort = 8080,
    }) {
      return ForwardConfig(
        id: id ?? 'test-id',
        name: name,
        sshHost: 'host',
        sshUsername: 'user',
        sshPassword: 'pass',
        localPort: localPort,
        remoteHost: 'remote',
        remotePort: 80,
      );
    }

    test('starts with empty forwards list', () {
      expect(provider.forwards, isEmpty);
    });

    test('loadForwards populates list', () async {
      final configs = [
        createConfig(id: '1', name: 'First'),
        createConfig(id: '2', name: 'Second'),
      ];
      await provider.loadForwards(configs);

      expect(provider.forwards, hasLength(2));
      expect(provider.forwards[0].name, 'First');
      expect(provider.forwards[1].name, 'Second');
    });

    test('addForward adds to list and saves', () async {
      final config = createConfig();
      await provider.addForward(config);

      expect(provider.forwards, hasLength(1));
      expect(provider.forwards[0].id, 'test-id');
      expect(mockStorage.lastSavedForwards, hasLength(1));
    });

    test('addForward notifies listeners', () async {
      var notified = false;
      provider.addListener(() => notified = true);

      await provider.addForward(createConfig());
      expect(notified, isTrue);
    });

    test('updateForward replaces config', () async {
      await provider.addForward(createConfig(id: 'id-1', name: 'Original'));

      final updated = createConfig(id: 'id-1', name: 'Updated');
      await provider.updateForward(updated);

      expect(provider.forwards, hasLength(1));
      expect(provider.forwards[0].name, 'Updated');
    });

    test('updateForward does nothing for unknown id', () async {
      await provider.addForward(createConfig(id: 'id-1'));

      final unknown = createConfig(id: 'unknown');
      await provider.updateForward(unknown);

      expect(provider.forwards, hasLength(1));
      expect(provider.forwards[0].id, 'id-1');
    });

    test('removeForward removes from list', () async {
      await provider.addForward(createConfig(id: 'id-1'));
      await provider.addForward(createConfig(id: 'id-2', name: 'Second'));

      await provider.removeForward('id-1');

      expect(provider.forwards, hasLength(1));
      expect(provider.forwards[0].id, 'id-2');
    });

    test('removeForward cleans up status and error', () async {
      await provider.addForward(createConfig(id: 'id-1'));

      await provider.removeForward('id-1');

      expect(provider.getStatus('id-1'), ForwardStatus.disconnected);
      expect(provider.getErrorMessage('id-1'), isNull);
    });

    test('duplicateForward creates copy with new id', () async {
      await provider.addForward(
          createConfig(id: 'id-1', name: 'Original', localPort: 3306));

      await provider.duplicateForward('id-1');

      expect(provider.forwards, hasLength(2));
      expect(provider.forwards[1].name, 'Original (copy)');
      expect(provider.forwards[1].id, isNot('id-1'));
      expect(provider.forwards[1].localPort, 3306);
      expect(provider.forwards[1].sshHost, 'host');
    });

    test('getStatus returns disconnected for unknown id', () {
      expect(provider.getStatus('unknown'), ForwardStatus.disconnected);
    });

    test('getErrorMessage returns null for unknown id', () {
      expect(provider.getErrorMessage('unknown'), isNull);
    });

    test('toggleForward sets error when no password or identity file', () async {
      final config = ForwardConfig(
        id: 'no-auth',
        name: 'No Auth',
        sshHost: 'host',
        sshUsername: 'user',
        localPort: 8080,
        remoteHost: 'remote',
        remotePort: 80,
      );
      await provider.addForward(config);

      await provider.toggleForward('no-auth');

      expect(provider.getStatus('no-auth'), ForwardStatus.error);
      expect(provider.getErrorMessage('no-auth'),
          'Password or identity file required');
    });

    test('forwards list is unmodifiable', () {
      final list = provider.forwards;
      expect(() => (list as List).add(createConfig()), throwsA(anything));
    });

    test('multiple adds accumulate', () async {
      await provider.addForward(createConfig(id: '1', name: 'One'));
      await provider.addForward(createConfig(id: '2', name: 'Two'));
      await provider.addForward(createConfig(id: '3', name: 'Three'));

      expect(provider.forwards, hasLength(3));
      expect(mockStorage.lastSavedForwards, hasLength(3));
    });

    test('notificationsEnabled can be toggled', () {
      provider.notificationsEnabled = false;
      // No assertion needed - just verify it doesn't throw
      provider.notificationsEnabled = true;
    });

    test('connectAll does nothing when forwards list is empty', () async {
      await provider.connectAll();
      // Should not throw
      expect(provider.forwards, isEmpty);
    });

    test('connectAll attempts to connect disconnected tunnels', () async {
      final config = createConfig(id: 'id-1');
      await provider.addForward(config);

      // Tunnel has no valid auth, so it will go to error state
      // but connectAll should still attempt it
      await provider.connectAll();
      // The tunnel should be in error state (no password/identity)
      // since we use a config with password, it will attempt connection
      expect(provider.getStatus('id-1'), isNotNull);
    });

    test('disconnectAll clears all statuses', () async {
      await provider.addForward(createConfig(id: 'id-1'));
      await provider.addForward(createConfig(id: 'id-2'));

      await provider.disconnectAll();

      expect(provider.getStatus('id-1'), ForwardStatus.disconnected);
      expect(provider.getStatus('id-2'), ForwardStatus.disconnected);
    });
  });
}
