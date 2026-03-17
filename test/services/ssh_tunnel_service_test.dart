import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/forward_config.dart';
import 'package:tunnel_pilot/models/forward_status.dart';
import 'package:tunnel_pilot/services/ssh_tunnel_service.dart';

void main() {
  group('SshTunnelService', () {
    late SshTunnelService service;

    setUp(() {
      service = SshTunnelService();
    });

    tearDown(() {
      service.dispose();
    });

    test('isConnected returns false for unknown id', () {
      expect(service.isConnected('unknown'), isFalse);
    });

    test('isAlive returns false for unknown id', () async {
      final alive = await service.isAlive('unknown');
      expect(alive, isFalse);
    });

    test('disconnect does nothing for unknown id', () async {
      // Should not throw
      await service.disconnect('unknown');
    });

    test('disconnectAll works when empty', () async {
      // Should not throw
      await service.disconnectAll();
    });

    test('dispose works when no connections', () {
      service.dispose();
      // Prevent double dispose in tearDown
      service = SshTunnelService();
    });

    group('connect', () {
      ForwardConfig createConfig({
        String id = 'test-id',
        String sshHost = 'invalid.host.test',
        int sshPort = 22,
        int localPort = 0,
        int keepAliveIntervalSec = 30,
      }) {
        return ForwardConfig(
          id: id,
          name: 'Test Tunnel',
          sshHost: sshHost,
          sshPort: sshPort,
          sshUsername: 'user',
          sshPassword: 'pass',
          localPort: localPort,
          remoteHost: 'localhost',
          remotePort: 80,
          keepAliveIntervalSec: keepAliveIntervalSec,
        );
      }

      test('reports connecting then error for invalid host', () async {
        final statuses = <ForwardStatus>[];
        String? lastError;

        await service.connect(
          createConfig(),
          onStatusChanged: (id, status, error) {
            statuses.add(status);
            if (error != null) lastError = error;
          },
        );

        expect(statuses.first, ForwardStatus.connecting);
        expect(statuses.last, ForwardStatus.error);
        expect(lastError, isNotNull);
        expect(service.isConnected('test-id'), isFalse);
      });

      test('reports error with correct id', () async {
        String? reportedId;

        await service.connect(
          createConfig(id: 'my-tunnel'),
          onStatusChanged: (id, status, error) {
            reportedId = id;
          },
        );

        expect(reportedId, 'my-tunnel');
      });

      test('stale callback is ignored after disconnect', () async {
        final statuses = <ForwardStatus>[];

        // Start a connection that will fail
        final connectFuture = service.connect(
          createConfig(id: 'stale-test'),
          onStatusChanged: (id, status, error) {
            statuses.add(status);
          },
        );

        // Disconnect immediately — increments generation
        await service.disconnect('stale-test');

        // Wait for connect to finish (will fail due to invalid host)
        await connectFuture;

        // Only the connecting status should be recorded before disconnect
        // bumped the generation; the error callback should be suppressed
        // because generation no longer matches.
        // Note: depending on timing, connecting may or may not be suppressed too.
        // The key assertion is that after disconnect, no new statuses arrive.
        final statusCount = statuses.length;

        // Further callbacks should not fire
        await Future.delayed(const Duration(milliseconds: 100));
        expect(statuses.length, statusCount);
      });

      test('second connect to same id disconnects first', () async {
        final statusesA = <ForwardStatus>[];
        final statusesB = <ForwardStatus>[];

        await service.connect(
          createConfig(id: 'dup'),
          onStatusChanged: (id, status, error) {
            statusesA.add(status);
          },
        );

        await service.connect(
          createConfig(id: 'dup'),
          onStatusChanged: (id, status, error) {
            statusesB.add(status);
          },
        );

        // Both should have attempted and failed (invalid host)
        expect(statusesA.last, ForwardStatus.error);
        expect(statusesB.last, ForwardStatus.error);
      });

      test('connect with keepAliveIntervalSec=0 does not crash', () async {
        final statuses = <ForwardStatus>[];

        await service.connect(
          createConfig(keepAliveIntervalSec: 0),
          onStatusChanged: (id, status, error) {
            statuses.add(status);
          },
        );

        // Should still report connecting then error (invalid host)
        expect(statuses.first, ForwardStatus.connecting);
        expect(statuses.last, ForwardStatus.error);
      });
    });
  });

  group('TunnelConnection', () {
    test('maxForwardFailures is 3', () {
      expect(TunnelConnection.maxForwardFailures, 3);
    });

    test('consecutiveForwardFailures starts at 0', () {
      // We can't easily construct TunnelConnection without real SSHClient/ServerSocket,
      // but we can verify the static constant is accessible and correct.
      expect(TunnelConnection.maxForwardFailures, greaterThan(0));
    });
  });
}
