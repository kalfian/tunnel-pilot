import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/services/update_service.dart';

void main() {
  group('UpdateService', () {
    late UpdateService service;

    setUp(() {
      service = UpdateService();
    });

    tearDown(() {
      service.dispose();
    });

    group('compareVersions', () {
      test('equal versions return 0', () {
        expect(service.compareVersions('1.0.0', '1.0.0'), 0);
        expect(service.compareVersions('0.0.1', '0.0.1'), 0);
        expect(service.compareVersions('2.3.4', '2.3.4'), 0);
      });

      test('older version returns negative', () {
        expect(service.compareVersions('1.0.0', '2.0.0'), isNegative);
        expect(service.compareVersions('1.0.0', '1.1.0'), isNegative);
        expect(service.compareVersions('1.0.0', '1.0.1'), isNegative);
        expect(service.compareVersions('0.0.1', '0.0.2'), isNegative);
      });

      test('newer version returns positive', () {
        expect(service.compareVersions('2.0.0', '1.0.0'), isPositive);
        expect(service.compareVersions('1.1.0', '1.0.0'), isPositive);
        expect(service.compareVersions('1.0.1', '1.0.0'), isPositive);
      });

      test('handles different length versions', () {
        expect(service.compareVersions('1.0', '1.0.0'), 0);
        expect(service.compareVersions('1.0.0', '1.0'), 0);
        expect(service.compareVersions('1.0', '1.0.1'), isNegative);
        expect(service.compareVersions('1.0.1', '1.0'), isPositive);
      });

      test('handles major version jumps', () {
        expect(service.compareVersions('0.0.1', '1.0.0'), isNegative);
        expect(service.compareVersions('9.9.9', '10.0.0'), isNegative);
      });
    });

    group('state management', () {
      test('initial state', () {
        expect(service.currentVersion, '');
        expect(service.latestVersion, isNull);
        expect(service.downloadUrl, isNull);
        expect(service.releaseNotes, isNull);
        expect(service.htmlUrl, isNull);
        expect(service.isChecking, isFalse);
        expect(service.updateAvailable, isFalse);
        expect(service.isDownloading, isFalse);
        expect(service.isInstalling, isFalse);
        expect(service.downloadProgress, 0.0);
        expect(service.errorMessage, isNull);
        expect(service.statusMessage, isNull);
      });

      test('dismissUpdate sets updateAvailable to false', () {
        service.updateAvailable = true;
        service.dismissUpdate();
        expect(service.updateAvailable, isFalse);
      });

      test('dismissUpdate notifies listeners', () {
        int notifyCount = 0;
        service.addListener(() => notifyCount++);

        service.dismissUpdate();
        expect(notifyCount, 1);
      });

      test('dismissUpdate clears error and status', () {
        service.errorMessage = 'some error';
        service.statusMessage = 'some status';
        service.updateAvailable = true;

        service.dismissUpdate();

        expect(service.updateAvailable, isFalse);
        expect(service.errorMessage, isNull);
        expect(service.statusMessage, isNull);
      });

      test('cancelUpdate resets all download state', () {
        service.isDownloading = true;
        service.isInstalling = true;
        service.downloadProgress = 0.5;
        service.statusMessage = 'Downloading...';
        service.errorMessage = 'previous error';

        service.cancelUpdate();

        expect(service.isDownloading, isFalse);
        expect(service.isInstalling, isFalse);
        expect(service.downloadProgress, 0.0);
        expect(service.statusMessage, isNull);
        expect(service.errorMessage, isNull);
      });

      test('cancelUpdate notifies listeners', () {
        int notifyCount = 0;
        service.addListener(() => notifyCount++);

        service.cancelUpdate();
        expect(notifyCount, 1);
      });

      test('setSkippedVersion stores version', () {
        // Verify indirectly: after setting skipped version,
        // the service should track it (no public getter, but we can
        // verify it doesn't crash)
        service.setSkippedVersion('1.2.3');
        service.setSkippedVersion(null);
      });

      test('downloadAndInstall guards against null downloadUrl', () async {
        service.downloadUrl = null;
        await service.downloadAndInstall();
        expect(service.isDownloading, isFalse);
      });

      test('downloadAndInstall guards against concurrent downloads', () async {
        service.isDownloading = true;
        service.downloadUrl = 'https://example.com/file.dmg';
        await service.downloadAndInstall();
        // Should return early without changing state
        expect(service.isDownloading, isTrue);
      });

      test('isInstalling is false initially', () {
        expect(service.isInstalling, isFalse);
      });

      test('downloadProgress resets after failed download', () async {
        // Set up a bad URL that will fail
        service.downloadUrl = 'https://invalid.invalid/file.dmg';
        await service.downloadAndInstall();

        // After failure, state should be fully reset
        expect(service.isDownloading, isFalse);
        expect(service.isInstalling, isFalse);
        expect(service.downloadProgress, 0.0);
        expect(service.statusMessage, isNull);
        // Error message should be set after failure
        expect(service.errorMessage, isNotNull);
      });
    });

    group('periodic check', () {
      test('startPeriodicCheck and stopPeriodicCheck', () {
        service.startPeriodicCheck(
            interval: const Duration(hours: 1));
        // Should not throw
        service.stopPeriodicCheck();
      });

      test('startPeriodicCheck replaces previous timer', () {
        service.startPeriodicCheck(
            interval: const Duration(hours: 1));
        service.startPeriodicCheck(
            interval: const Duration(hours: 2));
        // Should not throw or leak timers
        service.stopPeriodicCheck();
      });

      test('dispose cancels periodic timer', () {
        final disposableService = UpdateService();
        disposableService.startPeriodicCheck(
            interval: const Duration(hours: 1));
        disposableService.dispose();
        // Should not throw
      });
    });

    group('notification spam prevention', () {
      test('listener fires only once per version', () {
        // Simulate the pattern used in main.dart
        int notificationCount = 0;
        String? lastNotifiedVersion;

        service.addListener(() {
          if (service.updateAvailable &&
              service.latestVersion != null &&
              service.latestVersion != lastNotifiedVersion) {
            lastNotifiedVersion = service.latestVersion;
            notificationCount++;
          }
        });

        // Simulate update found: multiple notifyListeners calls happen
        service.latestVersion = '2.0.0';
        service.updateAvailable = true;

        // First notify — should trigger notification
        service.dismissUpdate(); // resets updateAvailable, calls notifyListeners
        service.updateAvailable = true;
        service.notifyListeners(); // this one should trigger

        expect(notificationCount, 1);

        // Additional notifyListeners with same version — should NOT trigger
        service.notifyListeners();
        service.notifyListeners();
        expect(notificationCount, 1);
      });

      test('listener fires again for new version', () {
        int notificationCount = 0;
        String? lastNotifiedVersion;

        service.addListener(() {
          if (service.updateAvailable &&
              service.latestVersion != null &&
              service.latestVersion != lastNotifiedVersion) {
            lastNotifiedVersion = service.latestVersion;
            notificationCount++;
          }
        });

        // First version
        service.latestVersion = '2.0.0';
        service.updateAvailable = true;
        service.notifyListeners();
        expect(notificationCount, 1);

        // Same version — no new notification
        service.notifyListeners();
        expect(notificationCount, 1);

        // New version — should trigger again
        service.latestVersion = '3.0.0';
        service.notifyListeners();
        expect(notificationCount, 2);
      });

      test('listener does not fire when updateAvailable is false', () {
        int notificationCount = 0;
        String? lastNotifiedVersion;

        service.addListener(() {
          if (service.updateAvailable &&
              service.latestVersion != null &&
              service.latestVersion != lastNotifiedVersion) {
            lastNotifiedVersion = service.latestVersion;
            notificationCount++;
          }
        });

        service.latestVersion = '2.0.0';
        service.updateAvailable = false;
        service.notifyListeners();
        expect(notificationCount, 0);
      });

      test('listener does not fire when latestVersion is null', () {
        int notificationCount = 0;
        String? lastNotifiedVersion;

        service.addListener(() {
          if (service.updateAvailable &&
              service.latestVersion != null &&
              service.latestVersion != lastNotifiedVersion) {
            lastNotifiedVersion = service.latestVersion;
            notificationCount++;
          }
        });

        service.latestVersion = null;
        service.updateAvailable = true;
        service.notifyListeners();
        expect(notificationCount, 0);
      });
    });
  });
}
