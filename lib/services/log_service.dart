import 'package:flutter/foundation.dart';

enum LogLevel { info, warning, error }

class LogEntry {
  final DateTime timestamp;
  final LogLevel level;
  final String tunnelName;
  final String message;

  LogEntry({
    required this.timestamp,
    required this.level,
    required this.tunnelName,
    required this.message,
  });

  String get formattedTime {
    final h = timestamp.hour.toString().padLeft(2, '0');
    final m = timestamp.minute.toString().padLeft(2, '0');
    final s = timestamp.second.toString().padLeft(2, '0');
    return '$h:$m:$s';
  }

  String get formattedLine =>
      '[$formattedTime] [${level.name.toUpperCase()}] [$tunnelName] $message';
}

class LogService extends ChangeNotifier {
  static const int maxLogs = 500;

  final List<LogEntry> _logs = [];

  List<LogEntry> get logs => List.unmodifiable(_logs);

  String get allLogsText => _logs.map((l) => l.formattedLine).join('\n');

  void add(LogLevel level, String tunnelName, String message) {
    _logs.insert(
      0,
      LogEntry(
        timestamp: DateTime.now(),
        level: level,
        tunnelName: tunnelName,
        message: message,
      ),
    );
    if (_logs.length > maxLogs) {
      _logs.removeLast();
    }
    notifyListeners();
  }

  void info(String tunnelName, String message) =>
      add(LogLevel.info, tunnelName, message);

  void warning(String tunnelName, String message) =>
      add(LogLevel.warning, tunnelName, message);

  void error(String tunnelName, String message) =>
      add(LogLevel.error, tunnelName, message);

  void clear() {
    _logs.clear();
    notifyListeners();
  }
}
