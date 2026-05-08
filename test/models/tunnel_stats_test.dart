import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/tunnel_stats.dart';

void main() {
  group('TunnelStats', () {
    test('default values', () {
      const stats = TunnelStats();
      expect(stats.activeConnections, 0);
      expect(stats.totalBytesUp, 0);
      expect(stats.totalBytesDown, 0);
      expect(stats.lastPingLatency, isNull);
      expect(stats.connectedSince, isNull);
    });

    test('uptime returns zero when connectedSince is null', () {
      const stats = TunnelStats();
      expect(stats.uptime, Duration.zero);
    });

    test('uptime returns positive duration when connectedSince is set', () {
      final stats = TunnelStats(
        connectedSince: DateTime.now().subtract(const Duration(minutes: 5)),
      );
      expect(stats.uptime.inMinutes, greaterThanOrEqualTo(4));
      expect(stats.uptime.inMinutes, lessThanOrEqualTo(6));
    });

    test('copyWith preserves values when no args given', () {
      final stats = TunnelStats(
        activeConnections: 3,
        totalBytesUp: 1024,
        totalBytesDown: 2048,
        lastPingLatency: const Duration(milliseconds: 42),
        connectedSince: DateTime(2025, 1, 1),
      );

      final copy = stats.copyWith();
      expect(copy.activeConnections, 3);
      expect(copy.totalBytesUp, 1024);
      expect(copy.totalBytesDown, 2048);
      expect(copy.lastPingLatency, const Duration(milliseconds: 42));
      expect(copy.connectedSince, DateTime(2025, 1, 1));
    });

    test('copyWith overrides specified fields', () {
      const stats = TunnelStats(activeConnections: 1, totalBytesUp: 100);
      final copy = stats.copyWith(activeConnections: 5, totalBytesDown: 999);
      expect(copy.activeConnections, 5);
      expect(copy.totalBytesUp, 100);
      expect(copy.totalBytesDown, 999);
    });

    test('equality compares all fields', () {
      final a = TunnelStats(
        activeConnections: 1,
        totalBytesUp: 100,
        totalBytesDown: 200,
        lastPingLatency: const Duration(milliseconds: 10),
        connectedSince: DateTime(2025, 6, 1),
      );
      final b = TunnelStats(
        activeConnections: 1,
        totalBytesUp: 100,
        totalBytesDown: 200,
        lastPingLatency: const Duration(milliseconds: 10),
        connectedSince: DateTime(2025, 6, 1),
      );
      expect(a, equals(b));
      expect(a.hashCode, equals(b.hashCode));
    });

    test('inequality when any field differs', () {
      const base = TunnelStats(activeConnections: 1, totalBytesUp: 100);
      expect(base, isNot(equals(base.copyWith(activeConnections: 2))));
      expect(base, isNot(equals(base.copyWith(totalBytesUp: 999))));
      expect(base, isNot(equals(base.copyWith(totalBytesDown: 1))));
    });
  });
}
