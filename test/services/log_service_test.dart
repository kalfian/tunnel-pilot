import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/services/log_service.dart';

void main() {
  group('LogEntry', () {
    test('formats time correctly', () {
      final entry = LogEntry(
        timestamp: DateTime(2024, 1, 15, 9, 5, 3),
        level: LogLevel.info,
        tunnelName: 'test',
        message: 'hello',
      );
      expect(entry.formattedTime, '09:05:03');
    });

    test('formats line with all parts', () {
      final entry = LogEntry(
        timestamp: DateTime(2024, 1, 15, 14, 30, 45),
        level: LogLevel.error,
        tunnelName: 'MyTunnel',
        message: 'Connection failed',
      );
      expect(entry.formattedLine,
          '[14:30:45] [ERROR] [MyTunnel] Connection failed');
    });

    test('formats warning level', () {
      final entry = LogEntry(
        timestamp: DateTime(2024, 1, 1, 0, 0, 0),
        level: LogLevel.warning,
        tunnelName: 'test',
        message: 'warn',
      );
      expect(entry.formattedLine, '[00:00:00] [WARNING] [test] warn');
    });
  });

  group('LogService', () {
    late LogService service;

    setUp(() {
      service = LogService();
    });

    test('starts with empty logs', () {
      expect(service.logs, isEmpty);
      expect(service.allLogsText, '');
    });

    test('add inserts log at beginning', () {
      service.add(LogLevel.info, 'tunnel1', 'first');
      service.add(LogLevel.info, 'tunnel2', 'second');

      expect(service.logs, hasLength(2));
      expect(service.logs[0].message, 'second');
      expect(service.logs[1].message, 'first');
    });

    test('info convenience method', () {
      service.info('t1', 'info msg');
      expect(service.logs, hasLength(1));
      expect(service.logs[0].level, LogLevel.info);
      expect(service.logs[0].tunnelName, 't1');
      expect(service.logs[0].message, 'info msg');
    });

    test('warning convenience method', () {
      service.warning('t1', 'warn msg');
      expect(service.logs, hasLength(1));
      expect(service.logs[0].level, LogLevel.warning);
    });

    test('error convenience method', () {
      service.error('t1', 'err msg');
      expect(service.logs, hasLength(1));
      expect(service.logs[0].level, LogLevel.error);
    });

    test('notifies listeners on add', () {
      int notifyCount = 0;
      service.addListener(() => notifyCount++);

      service.info('t', 'msg');
      expect(notifyCount, 1);

      service.warning('t', 'msg');
      expect(notifyCount, 2);
    });

    test('enforces max log limit', () {
      for (int i = 0; i < LogService.maxLogs + 50; i++) {
        service.info('t', 'msg $i');
      }
      expect(service.logs.length, LogService.maxLogs);
    });

    test('oldest logs are removed when exceeding max', () {
      for (int i = 0; i < LogService.maxLogs + 5; i++) {
        service.info('t', 'msg $i');
      }
      // Most recent should be at index 0
      expect(service.logs[0].message, 'msg ${LogService.maxLogs + 4}');
    });

    test('clear removes all logs', () {
      service.info('t', 'msg1');
      service.info('t', 'msg2');
      service.clear();

      expect(service.logs, isEmpty);
    });

    test('clear notifies listeners', () {
      int notifyCount = 0;
      service.addListener(() => notifyCount++);

      service.clear();
      expect(notifyCount, 1);
    });

    test('allLogsText joins formatted lines', () {
      service.info('t1', 'first');
      service.info('t2', 'second');

      final text = service.allLogsText;
      expect(text, contains('[INFO] [t2] second'));
      expect(text, contains('[INFO] [t1] first'));
      expect(text.split('\n'), hasLength(2));
    });

    test('logs list is unmodifiable', () {
      service.info('t', 'msg');
      final logs = service.logs;
      expect(
        () => (logs as List).add(LogEntry(
          timestamp: DateTime.now(),
          level: LogLevel.info,
          tunnelName: 't',
          message: 'x',
        )),
        throwsA(anything),
      );
    });

    test('logs list is cached until dirty', () {
      service.info('t', 'msg');
      final logs1 = service.logs;
      final logs2 = service.logs;
      expect(identical(logs1, logs2), isTrue);

      // After add, cache should be invalidated
      service.info('t', 'msg2');
      final logs3 = service.logs;
      expect(identical(logs1, logs3), isFalse);
    });
  });
}
