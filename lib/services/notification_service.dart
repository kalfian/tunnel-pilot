import 'dart:io';

import 'package:flutter/foundation.dart';

class NotificationService {
  Future<void> init() async {
    // No setup needed for osascript-based notifications
  }

  Future<void> show(String title, String body) async {
    if (Platform.isMacOS) {
      try {
        final escapedTitle = title.replaceAll('"', '\\"');
        final escapedBody = body.replaceAll('"', '\\"');
        await Process.run('osascript', [
          '-e',
          'display notification "$escapedBody" with title "$escapedTitle"',
        ]);
      } catch (e) {
        debugPrint('Notification failed: $e');
      }
    }
  }

  Future<void> showConnected(String name) async {
    await show('Tunnel Connected', '$name is now active.');
  }

  Future<void> showDisconnected(String name) async {
    await show('Tunnel Disconnected', '$name has been disconnected.');
  }

  Future<void> showError(String name, String error) async {
    await show('Tunnel Error', '$name: $error');
  }
}
