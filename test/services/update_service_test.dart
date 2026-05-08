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

      test('pre-release is lower than release with same core', () {
        expect(service.compareVersions('1.2.7-beta', '1.2.7'), isNegative);
        expect(service.compareVersions('1.2.7', '1.2.7-beta'), isPositive);
      });

      test('compares two pre-releases lexicographically', () {
        expect(
            service.compareVersions('1.2.7-alpha', '1.2.7-beta'), isNegative);
        expect(
            service.compareVersions('1.2.7-rc.1', '1.2.7-rc.2'), isNegative);
        expect(service.compareVersions('1.2.7-beta', '1.2.7-beta'), 0);
      });

      test('ignores build metadata', () {
        expect(service.compareVersions('1.2.7+build.1', '1.2.7'), 0);
        expect(service.compareVersions('1.2.7+a', '1.2.7+b'), 0);
        expect(service.compareVersions('1.2.7-beta+a', '1.2.7-beta+b'), 0);
      });

      test('core version wins over pre-release suffix', () {
        expect(service.compareVersions('1.2.8-beta', '1.2.7'), isPositive);
        expect(service.compareVersions('1.2.7', '1.2.8-beta'), isNegative);
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
        expect(service.isUpToDate, isFalse);
        expect(service.isDownloading, isFalse);
        expect(service.isInstalling, isFalse);
        expect(service.downloadProgress, 0.0);
        expect(service.downloadedBytes, 0);
        expect(service.totalBytes, 0);
        expect(service.errorMessage, isNull);
        expect(service.checkError, isNull);
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
        expect(service.isDownloading, isTrue);
      });

      test('isInstalling is false initially', () {
        expect(service.isInstalling, isFalse);
      });

      test('downloadProgress resets after failed download', () async {
        service.downloadUrl = 'https://invalid.invalid/file.dmg';
        await service.downloadAndInstall();

        expect(service.isDownloading, isFalse);
        expect(service.isInstalling, isFalse);
        expect(service.downloadProgress, 0.0);
        expect(service.downloadedBytes, 0);
        expect(service.totalBytes, 0);
        expect(service.statusMessage, isNull);
        expect(service.errorMessage, isNotNull);
      });

      test('clearCheckStatus resets check feedback', () {
        service.clearCheckStatus();
        expect(service.isUpToDate, isFalse);
        expect(service.checkError, isNull);
      });

      test('downloadSizeText returns empty when totalBytes is 0', () {
        expect(service.downloadSizeText, '');
      });
    });

    group('periodic check', () {
      test('startPeriodicCheck and stopPeriodicCheck', () {
        service.startPeriodicCheck(interval: const Duration(hours: 1));
        service.stopPeriodicCheck();
      });

      test('startPeriodicCheck replaces previous timer', () {
        service.startPeriodicCheck(interval: const Duration(hours: 1));
        service.startPeriodicCheck(interval: const Duration(hours: 2));
        service.stopPeriodicCheck();
      });

      test('dispose cancels periodic timer', () {
        final disposableService = UpdateService();
        disposableService.startPeriodicCheck(
            interval: const Duration(hours: 1));
        disposableService.dispose();
      });
    });

    group('notification spam prevention', () {
      test('listener fires only once per version', () {
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
        service.updateAvailable = true;

        service.dismissUpdate();
        service.updateAvailable = true;
        service.notifyListeners();

        expect(notificationCount, 1);

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

        service.latestVersion = '2.0.0';
        service.updateAvailable = true;
        service.notifyListeners();
        expect(notificationCount, 1);

        service.notifyListeners();
        expect(notificationCount, 1);

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
