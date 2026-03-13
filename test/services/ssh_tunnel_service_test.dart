import 'package:flutter_test/flutter_test.dart';
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
  });
}
