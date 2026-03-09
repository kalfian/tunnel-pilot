import 'dart:io';
import 'dart:convert';

import 'package:path_provider/path_provider.dart';

/// Ensures only one instance of the app runs at a time.
/// Uses a local TCP server + lock file for cross-platform IPC.
/// When a second instance launches, it sends "show" to the first and exits.
class SingleInstanceService {
  ServerSocket? _server;
  void Function()? onSecondInstance;

  /// Returns true if this is the first (primary) instance.
  /// Returns false if another instance is already running (message sent to it).
  Future<bool> ensureSingleInstance() async {
    final lockFile = await _lockFile();

    // Try connecting to an existing instance
    if (lockFile.existsSync()) {
      try {
        final port = int.parse(lockFile.readAsStringSync().trim());
        final socket = await Socket.connect('127.0.0.1', port,
            timeout: const Duration(seconds: 1));
        socket.writeln('show');
        await socket.flush();
        await socket.close();
        return false; // Another instance is running
      } catch (_) {
        // Stale lock file — previous instance crashed
        lockFile.deleteSync();
      }
    }

    // Start listening as the primary instance
    _server = await ServerSocket.bind('127.0.0.1', 0);
    lockFile.writeAsStringSync('${_server!.port}');

    _server!.listen((socket) {
      socket.cast<List<int>>().transform(utf8.decoder).listen((data) {
        if (data.trim() == 'show') {
          onSecondInstance?.call();
        }
        socket.close();
      });
    });

    return true;
  }

  Future<void> dispose() async {
    await _server?.close();
    final lockFile = await _lockFile();
    if (lockFile.existsSync()) {
      lockFile.deleteSync();
    }
  }

  Future<File> _lockFile() async {
    final dir = await getApplicationSupportDirectory();
    return File('${dir.path}${Platform.pathSeparator}.tunnel_pilot.lock');
  }
}
