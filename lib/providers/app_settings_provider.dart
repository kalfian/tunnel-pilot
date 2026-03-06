import 'package:flutter/material.dart';

import '../models/app_settings.dart';
import '../services/startup_service.dart';
import '../services/storage_service.dart';

class AppSettingsProvider extends ChangeNotifier {
  final StorageService _storage;
  final StartupService _startup;
  AppSettings _settings;

  AppSettingsProvider({
    required StorageService storage,
    required StartupService startup,
    required AppSettings settings,
  })  : _storage = storage,
        _startup = startup,
        _settings = settings;

  AppSettings get settings => _settings;

  bool get launchAtLogin => _settings.launchAtLogin;
  bool get showNotifications => _settings.showNotifications;
  String get themeMode => _settings.themeMode;

  ThemeMode get themeModeEnum {
    switch (_settings.themeMode) {
      case 'light':
        return ThemeMode.light;
      case 'dark':
        return ThemeMode.dark;
      default:
        return ThemeMode.system;
    }
  }

  Future<void> setLaunchAtLogin(bool value) async {
    _settings.launchAtLogin = value;
    await _startup.setEnabled(value);
    await _storage.saveSettings(_settings);
    notifyListeners();
  }

  Future<void> setShowNotifications(bool value) async {
    _settings.showNotifications = value;
    await _storage.saveSettings(_settings);
    notifyListeners();
  }

  Future<void> setThemeMode(String mode) async {
    _settings.themeMode = mode;
    await _storage.saveSettings(_settings);
    notifyListeners();
  }
}
