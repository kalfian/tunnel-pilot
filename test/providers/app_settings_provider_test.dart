import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/app_settings.dart';
import 'package:tunnel_pilot/providers/app_settings_provider.dart';
import 'package:tunnel_pilot/services/startup_service.dart';
import 'package:tunnel_pilot/services/storage_service.dart';

class MockStorageService extends StorageService {
  AppSettings? lastSavedSettings;

  @override
  Future<void> saveSettings(AppSettings settings) async {
    lastSavedSettings = settings;
  }
}

class MockStartupService extends StartupService {
  bool? lastEnabledValue;

  @override
  Future<void> setEnabled(bool enabled) async {
    lastEnabledValue = enabled;
  }
}

void main() {
  group('AppSettingsProvider', () {
    late MockStorageService mockStorage;
    late MockStartupService mockStartup;
    late AppSettingsProvider provider;

    setUp(() {
      mockStorage = MockStorageService();
      mockStartup = MockStartupService();
      provider = AppSettingsProvider(
        storage: mockStorage,
        startup: mockStartup,
        settings: AppSettings(),
      );
    });

    test('exposes default settings', () {
      expect(provider.launchAtLogin, isTrue);
      expect(provider.showNotifications, isTrue);
      expect(provider.themeMode, 'system');
      expect(provider.autoReconnect, isTrue);
      expect(provider.autoReconnectDelaySec, 5);
      expect(provider.autoReconnectMaxRetries, 3);
      expect(provider.showInDock, isFalse);
      expect(provider.autoCheckUpdates, isTrue);
      expect(provider.lastSkippedVersion, isNull);
    });

    test('exposes custom settings', () {
      provider = AppSettingsProvider(
        storage: mockStorage,
        startup: mockStartup,
        settings: AppSettings(
          launchAtLogin: false,
          showNotifications: false,
          themeMode: 'dark',
          autoReconnect: false,
          autoReconnectDelaySec: 10,
          autoReconnectMaxRetries: 5,
          showInDock: true,
          autoCheckUpdates: false,
          lastSkippedVersion: '1.0.0',
        ),
      );

      expect(provider.launchAtLogin, isFalse);
      expect(provider.showNotifications, isFalse);
      expect(provider.themeMode, 'dark');
      expect(provider.autoReconnect, isFalse);
      expect(provider.autoReconnectDelaySec, 10);
      expect(provider.autoReconnectMaxRetries, 5);
      expect(provider.showInDock, isTrue);
      expect(provider.autoCheckUpdates, isFalse);
      expect(provider.lastSkippedVersion, '1.0.0');
    });

    group('themeModeEnum', () {
      test('returns ThemeMode.system for "system"', () {
        expect(provider.themeModeEnum, ThemeMode.system);
      });

      test('returns ThemeMode.light for "light"', () async {
        await provider.setThemeMode('light');
        expect(provider.themeModeEnum, ThemeMode.light);
      });

      test('returns ThemeMode.dark for "dark"', () async {
        await provider.setThemeMode('dark');
        expect(provider.themeModeEnum, ThemeMode.dark);
      });

      test('returns ThemeMode.system for unknown value', () async {
        await provider.setThemeMode('unknown');
        expect(provider.themeModeEnum, ThemeMode.system);
      });
    });

    group('setters', () {
      test('setLaunchAtLogin updates value, saves, and notifies', () async {
        int notifyCount = 0;
        provider.addListener(() => notifyCount++);

        await provider.setLaunchAtLogin(false);

        expect(provider.launchAtLogin, isFalse);
        expect(mockStorage.lastSavedSettings?.launchAtLogin, isFalse);
        expect(mockStartup.lastEnabledValue, isFalse);
        expect(notifyCount, 1);
      });

      test('setShowNotifications updates and saves', () async {
        await provider.setShowNotifications(false);
        expect(provider.showNotifications, isFalse);
        expect(mockStorage.lastSavedSettings?.showNotifications, isFalse);
      });

      test('setThemeMode updates and saves', () async {
        await provider.setThemeMode('dark');
        expect(provider.themeMode, 'dark');
        expect(mockStorage.lastSavedSettings?.themeMode, 'dark');
      });

      test('setAutoReconnect updates and saves', () async {
        await provider.setAutoReconnect(false);
        expect(provider.autoReconnect, isFalse);
        expect(mockStorage.lastSavedSettings?.autoReconnect, isFalse);
      });

      test('setAutoReconnectDelaySec updates and saves', () async {
        await provider.setAutoReconnectDelaySec(15);
        expect(provider.autoReconnectDelaySec, 15);
        expect(mockStorage.lastSavedSettings?.autoReconnectDelaySec, 15);
      });

      test('setAutoReconnectMaxRetries updates and saves', () async {
        await provider.setAutoReconnectMaxRetries(10);
        expect(provider.autoReconnectMaxRetries, 10);
        expect(mockStorage.lastSavedSettings?.autoReconnectMaxRetries, 10);
      });

      test('setShowInDock updates and saves', () async {
        await provider.setShowInDock(true);
        expect(provider.showInDock, isTrue);
        expect(mockStorage.lastSavedSettings?.showInDock, isTrue);
      });

      test('setAutoCheckUpdates updates and saves', () async {
        await provider.setAutoCheckUpdates(false);
        expect(provider.autoCheckUpdates, isFalse);
        expect(mockStorage.lastSavedSettings?.autoCheckUpdates, isFalse);
      });

      test('setLastSkippedVersion updates and saves', () async {
        await provider.setLastSkippedVersion('2.0.0');
        expect(provider.lastSkippedVersion, '2.0.0');
        expect(mockStorage.lastSavedSettings?.lastSkippedVersion, '2.0.0');
      });

      test('setLastSkippedVersion with null', () async {
        await provider.setLastSkippedVersion('1.0.0');
        await provider.setLastSkippedVersion(null);
        expect(provider.lastSkippedVersion, isNull);
        expect(mockStorage.lastSavedSettings?.lastSkippedVersion, isNull);
      });

      test('all setters notify listeners', () async {
        int notifyCount = 0;
        provider.addListener(() => notifyCount++);

        await provider.setLaunchAtLogin(false);
        await provider.setShowNotifications(false);
        await provider.setThemeMode('dark');
        await provider.setAutoReconnect(false);
        await provider.setAutoReconnectDelaySec(10);
        await provider.setAutoReconnectMaxRetries(5);
        await provider.setShowInDock(true);
        await provider.setAutoCheckUpdates(false);
        await provider.setLastSkippedVersion('1.0.0');

        expect(notifyCount, 9);
      });
    });

    test('settings getter returns current settings object', () {
      final settings = provider.settings;
      expect(settings, isA<AppSettings>());
      expect(settings.launchAtLogin, isTrue);
    });
  });
}
