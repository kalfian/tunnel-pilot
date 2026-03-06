import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/forward_status.dart';

void main() {
  group('ForwardStatus', () {
    test('has all expected values', () {
      expect(ForwardStatus.values, hasLength(4));
      expect(ForwardStatus.values, contains(ForwardStatus.disconnected));
      expect(ForwardStatus.values, contains(ForwardStatus.connecting));
      expect(ForwardStatus.values, contains(ForwardStatus.connected));
      expect(ForwardStatus.values, contains(ForwardStatus.error));
    });

    test('enum names are correct', () {
      expect(ForwardStatus.disconnected.name, 'disconnected');
      expect(ForwardStatus.connecting.name, 'connecting');
      expect(ForwardStatus.connected.name, 'connected');
      expect(ForwardStatus.error.name, 'error');
    });
  });
}
