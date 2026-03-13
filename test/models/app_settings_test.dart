import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/app_settings.dart';

void main() {
  group('AppSettings', () {
    test('creates with default values', () {
      final settings = AppSettings();
      expect(settings.launchAtLogin, isTrue);
      expect(settings.showNotifications, isTrue);
      expect(settings.themeMode, 'system');
      expect(settings.autoReconnect, isTrue);
      expect(settings.autoReconnectDelaySec, 5);
      expect(settings.autoReconnectMaxRetries, 3);
      expect(settings.showInDock, isFalse);
      expect(settings.autoCheckUpdates, isTrue);
      expect(settings.lastSkippedVersion, isNull);
    });

    test('creates with custom values', () {
      final settings = AppSettings(
        launchAtLogin: false,
        showNotifications: false,
        themeMode: 'dark',
        autoReconnect: false,
        autoReconnectDelaySec: 10,
        autoReconnectMaxRetries: 5,
        showInDock: true,
        autoCheckUpdates: false,
        lastSkippedVersion: '1.2.3',
      );
      expect(settings.launchAtLogin, isFalse);
      expect(settings.showNotifications, isFalse);
      expect(settings.themeMode, 'dark');
      expect(settings.autoReconnect, isFalse);
      expect(settings.autoReconnectDelaySec, 10);
      expect(settings.autoReconnectMaxRetries, 5);
      expect(settings.showInDock, isTrue);
      expect(settings.autoCheckUpdates, isFalse);
      expect(settings.lastSkippedVersion, '1.2.3');
    });

    group('toJson', () {
      test('serializes default values', () {
        final settings = AppSettings();
        final json = settings.toJson();
        expect(json['launchAtLogin'], isTrue);
        expect(json['showNotifications'], isTrue);
        expect(json['themeMode'], 'system');
        expect(json['autoReconnect'], isTrue);
        expect(json['autoReconnectDelaySec'], 5);
        expect(json['autoReconnectMaxRetries'], 3);
        expect(json['showInDock'], isFalse);
        expect(json['autoCheckUpdates'], isTrue);
        expect(json['lastSkippedVersion'], isNull);
      });

      test('serializes custom values', () {
        final settings = AppSettings(
          launchAtLogin: false,
          showNotifications: false,
          themeMode: 'light',
          autoReconnect: false,
          autoReconnectDelaySec: 15,
          autoReconnectMaxRetries: 10,
          showInDock: true,
          autoCheckUpdates: false,
          lastSkippedVersion: '2.0.0',
        );
        final json = settings.toJson();
        expect(json['launchAtLogin'], isFalse);
        expect(json['showNotifications'], isFalse);
        expect(json['themeMode'], 'light');
        expect(json['autoReconnect'], isFalse);
        expect(json['autoReconnectDelaySec'], 15);
        expect(json['autoReconnectMaxRetries'], 10);
        expect(json['showInDock'], isTrue);
        expect(json['autoCheckUpdates'], isFalse);
        expect(json['lastSkippedVersion'], '2.0.0');
      });
    });

    group('fromJson', () {
      test('deserializes all fields', () {
        final json = {
          'launchAtLogin': false,
          'showNotifications': false,
          'themeMode': 'dark',
          'autoReconnect': false,
          'autoReconnectDelaySec': 10,
          'autoReconnectMaxRetries': 5,
          'showInDock': true,
          'autoCheckUpdates': false,
          'lastSkippedVersion': '1.0.0',
        };
        final settings = AppSettings.fromJson(json);
        expect(settings.launchAtLogin, isFalse);
        expect(settings.showNotifications, isFalse);
        expect(settings.themeMode, 'dark');
        expect(settings.autoReconnect, isFalse);
        expect(settings.autoReconnectDelaySec, 10);
        expect(settings.autoReconnectMaxRetries, 5);
        expect(settings.showInDock, isTrue);
        expect(settings.autoCheckUpdates, isFalse);
        expect(settings.lastSkippedVersion, '1.0.0');
      });

      test('uses defaults for missing fields', () {
        final settings = AppSettings.fromJson({});
        expect(settings.launchAtLogin, isTrue);
        expect(settings.showNotifications, isTrue);
        expect(settings.themeMode, 'system');
        expect(settings.autoReconnect, isTrue);
        expect(settings.autoReconnectDelaySec, 5);
        expect(settings.autoReconnectMaxRetries, 3);
        expect(settings.showInDock, isFalse);
        expect(settings.autoCheckUpdates, isTrue);
        expect(settings.lastSkippedVersion, isNull);
      });

      test('handles null values with defaults', () {
        final json = {
          'launchAtLogin': null,
          'showNotifications': null,
          'themeMode': null,
          'autoReconnect': null,
          'autoReconnectDelaySec': null,
          'autoReconnectMaxRetries': null,
          'showInDock': null,
          'autoCheckUpdates': null,
          'lastSkippedVersion': null,
        };
        final settings = AppSettings.fromJson(json);
        expect(settings.launchAtLogin, isTrue);
        expect(settings.showNotifications, isTrue);
        expect(settings.themeMode, 'system');
        expect(settings.autoReconnect, isTrue);
        expect(settings.autoReconnectDelaySec, 5);
        expect(settings.autoReconnectMaxRetries, 3);
        expect(settings.showInDock, isFalse);
        expect(settings.autoCheckUpdates, isTrue);
        expect(settings.lastSkippedVersion, isNull);
      });
    });

    group('roundtrip', () {
      test('toJson -> fromJson preserves all data', () {
        final original = AppSettings(
          launchAtLogin: false,
          showNotifications: false,
          themeMode: 'dark',
          autoReconnect: false,
          autoReconnectDelaySec: 20,
          autoReconnectMaxRetries: 7,
          showInDock: true,
          autoCheckUpdates: false,
          lastSkippedVersion: '3.0.0',
        );
        final json = original.toJson();
        final restored = AppSettings.fromJson(json);

        expect(restored.launchAtLogin, original.launchAtLogin);
        expect(restored.showNotifications, original.showNotifications);
        expect(restored.themeMode, original.themeMode);
        expect(restored.autoReconnect, original.autoReconnect);
        expect(restored.autoReconnectDelaySec, original.autoReconnectDelaySec);
        expect(
            restored.autoReconnectMaxRetries, original.autoReconnectMaxRetries);
        expect(restored.showInDock, original.showInDock);
        expect(restored.autoCheckUpdates, original.autoCheckUpdates);
        expect(restored.lastSkippedVersion, original.lastSkippedVersion);
      });

      test('roundtrip with null lastSkippedVersion', () {
        final original = AppSettings(lastSkippedVersion: null);
        final restored = AppSettings.fromJson(original.toJson());
        expect(restored.lastSkippedVersion, isNull);
      });
    });

    test('fields are mutable', () {
      final settings = AppSettings();
      settings.launchAtLogin = false;
      settings.showNotifications = false;
      settings.themeMode = 'light';
      settings.autoReconnect = false;
      settings.autoReconnectDelaySec = 30;
      settings.autoReconnectMaxRetries = 1;
      settings.showInDock = true;
      settings.autoCheckUpdates = false;
      settings.lastSkippedVersion = '1.0.0';

      expect(settings.launchAtLogin, isFalse);
      expect(settings.showNotifications, isFalse);
      expect(settings.themeMode, 'light');
      expect(settings.autoReconnect, isFalse);
      expect(settings.autoReconnectDelaySec, 30);
      expect(settings.autoReconnectMaxRetries, 1);
      expect(settings.showInDock, isTrue);
      expect(settings.autoCheckUpdates, isFalse);
      expect(settings.lastSkippedVersion, '1.0.0');
    });
  });
}
