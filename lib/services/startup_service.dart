import 'package:launch_at_startup/launch_at_startup.dart';

class StartupService {
  void init(String appName, String packageName) {
    launchAtStartup.setup(
      appName: appName,
      appPath: packageName,
    );
  }

  Future<bool> get isEnabled => launchAtStartup.isEnabled();

  Future<void> enable() async {
    await launchAtStartup.enable();
  }

  Future<void> disable() async {
    await launchAtStartup.disable();
  }

  Future<void> setEnabled(bool enabled) async {
    if (enabled) {
      await enable();
    } else {
      await disable();
    }
  }
}
