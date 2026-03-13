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
  bool get autoReconnect => _settings.autoReconnect;
  int get autoReconnectDelaySec => _settings.autoReconnectDelaySec;
  int get autoReconnectMaxRetries => _settings.autoReconnectMaxRetries;
  bool get showInDock => _settings.showInDock;
  bool get autoCheckUpdates => _settings.autoCheckUpdates;
  String? get lastSkippedVersion => _settings.lastSkippedVersion;

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

  Future<void> setAutoReconnect(bool value) async {
    _settings.autoReconnect = value;
    await _storage.saveSettings(_settings);
    notifyListeners();
  }

  Future<void> setAutoReconnectDelaySec(int value) async {
    _settings.autoReconnectDelaySec = value;
    await _storage.saveSettings(_settings);
    notifyListeners();
  }

  Future<void> setAutoReconnectMaxRetries(int value) async {
    _settings.autoReconnectMaxRetries = value;
    await _storage.saveSettings(_settings);
    notifyListeners();
  }

  Future<void> setShowInDock(bool value) async {
    _settings.showInDock = value;
    await _storage.saveSettings(_settings);
    notifyListeners();
  }

  Future<void> setAutoCheckUpdates(bool value) async {
    _settings.autoCheckUpdates = value;
    await _storage.saveSettings(_settings);
    notifyListeners();
  }

  Future<void> setLastSkippedVersion(String? value) async {
    _settings.lastSkippedVersion = value;
    await _storage.saveSettings(_settings);
    notifyListeners();
  }
}
