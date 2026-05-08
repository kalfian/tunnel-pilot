import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/services/counting_socket_wrapper.dart';

void main() {
  group('CountingSocketWrapper', () {
    test('starts with zero counters', () {
      final wrapper = CountingSocketWrapper();
      expect(wrapper.bytesUp, 0);
      expect(wrapper.bytesDown, 0);
    });

    test('bytesDown increments correctly', () {
      final wrapper = CountingSocketWrapper();
      wrapper.bytesDown += [1, 2, 3].length;
      wrapper.bytesDown += [4, 5].length;
      expect(wrapper.bytesDown, 5);
    });

    test('bytesUp increments correctly', () {
      final wrapper = CountingSocketWrapper();
      wrapper.bytesUp += [10, 20, 30, 40].length;
      wrapper.bytesUp += [50].length;
      expect(wrapper.bytesUp, 5);
    });

    test('counters are independent', () {
      final wrapper = CountingSocketWrapper();
      wrapper.bytesUp += 100;
      wrapper.bytesDown += 200;
      expect(wrapper.bytesUp, 100);
      expect(wrapper.bytesDown, 200);
    });

    test('accumulates across many increments', () {
      final wrapper = CountingSocketWrapper();
      for (var i = 0; i < 100; i++) {
        wrapper.bytesDown += 10;
      }
      expect(wrapper.bytesDown, 1000);
    });

    test('multiple wrappers track independently', () {
      final a = CountingSocketWrapper();
      final b = CountingSocketWrapper();
      a.bytesUp += 50;
      b.bytesUp += 30;
      expect(a.bytesUp, 50);
      expect(b.bytesUp, 30);
    });
  });
}
