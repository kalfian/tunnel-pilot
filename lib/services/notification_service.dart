import 'package:local_notifier/local_notifier.dart';

class NotificationService {
  Future<void> init() async {
    await localNotifier.setup(appName: 'Tunnel Pilot');
  }

  Future<void> show(String title, String body) async {
    final notification = LocalNotification(
      title: title,
      body: body,
    );
    await notification.show();
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
