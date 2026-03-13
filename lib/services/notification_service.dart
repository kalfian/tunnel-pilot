import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:local_notifier/local_notifier.dart';

class NotificationService {
  static const _macChannel = MethodChannel('native_notifications');

  Future<void> init() async {
    if (!Platform.isMacOS) {
      await localNotifier.setup(appName: 'Tunnel Pilot');
    }
  }

  Future<void> show(String title, String body) async {
    try {
      if (Platform.isMacOS) {
        await _macChannel.invokeMethod('show', {
          'title': title,
          'body': body,
        });
      } else {
        final notification = LocalNotification(
          title: title,
          body: body,
        );
        await notification.show();
      }
    } catch (e) {
      debugPrint('Notification failed: $e');
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
