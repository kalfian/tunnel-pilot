import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/app_settings.dart';

void main() {
  group('AppSettings', () {
    test('creates with default values', () {
      final settings = AppSettings();
      expect(settings.launchAtLogin, isFalse);
      expect(settings.showNotifications, isTrue);
    });

    test('creates with custom values', () {
      final settings = AppSettings(
        launchAtLogin: true,
        showNotifications: false,
      );
      expect(settings.launchAtLogin, isTrue);
      expect(settings.showNotifications, isFalse);
    });

    group('toJson', () {
      test('serializes default values', () {
        final settings = AppSettings();
        final json = settings.toJson();
        expect(json['launchAtLogin'], isFalse);
        expect(json['showNotifications'], isTrue);
      });

      test('serializes custom values', () {
        final settings = AppSettings(
          launchAtLogin: true,
          showNotifications: false,
        );
        final json = settings.toJson();
        expect(json['launchAtLogin'], isTrue);
        expect(json['showNotifications'], isFalse);
      });
    });

    group('fromJson', () {
      test('deserializes all fields', () {
        final json = {
          'launchAtLogin': true,
          'showNotifications': false,
        };
        final settings = AppSettings.fromJson(json);
        expect(settings.launchAtLogin, isTrue);
        expect(settings.showNotifications, isFalse);
      });

      test('uses defaults for missing fields', () {
        final settings = AppSettings.fromJson({});
        expect(settings.launchAtLogin, isFalse);
        expect(settings.showNotifications, isTrue);
      });

      test('handles null values with defaults', () {
        final json = {
          'launchAtLogin': null,
          'showNotifications': null,
        };
        final settings = AppSettings.fromJson(json);
        expect(settings.launchAtLogin, isFalse);
        expect(settings.showNotifications, isTrue);
      });
    });

    group('roundtrip', () {
      test('toJson -> fromJson preserves data', () {
        final original = AppSettings(
          launchAtLogin: true,
          showNotifications: false,
        );
        final json = original.toJson();
        final restored = AppSettings.fromJson(json);

        expect(restored.launchAtLogin, original.launchAtLogin);
        expect(restored.showNotifications, original.showNotifications);
      });
    });

    test('fields are mutable', () {
      final settings = AppSettings();
      settings.launchAtLogin = true;
      settings.showNotifications = false;
      expect(settings.launchAtLogin, isTrue);
      expect(settings.showNotifications, isFalse);
    });
  });
}
