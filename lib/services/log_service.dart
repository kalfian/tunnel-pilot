import 'package:flutter/foundation.dart';

enum LogLevel { info, warning, error }

class LogEntry {
  final LogLevel level;
  final String tunnelName;
  final String message;
  final String formattedTime;
  final String formattedLine;

  LogEntry({
    required DateTime timestamp,
    required this.level,
    required this.tunnelName,
    required this.message,
  })  : formattedTime =
            '${timestamp.hour.toString().padLeft(2, '0')}:${timestamp.minute.toString().padLeft(2, '0')}:${timestamp.second.toString().padLeft(2, '0')}',
        formattedLine =
            '[${timestamp.hour.toString().padLeft(2, '0')}:${timestamp.minute.toString().padLeft(2, '0')}:${timestamp.second.toString().padLeft(2, '0')}] [${level.name.toUpperCase()}] [$tunnelName] $message';
}

class LogService extends ChangeNotifier {
  static const int maxLogs = 500;

  final List<LogEntry> _logs = [];

  List<LogEntry> _unmodifiableLogs = const [];
  bool _logsDirty = true;

  List<LogEntry> get logs {
    if (_logsDirty) {
      _unmodifiableLogs = List.unmodifiable(_logs);
      _logsDirty = false;
    }
    return _unmodifiableLogs;
  }

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
    _logsDirty = true;
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
    _logsDirty = true;
    notifyListeners();
  }
}
