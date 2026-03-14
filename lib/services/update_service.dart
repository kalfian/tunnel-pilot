import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';

class UpdateService extends ChangeNotifier {
  static const String _repoOwner = 'kalfian';
  static const String _repoName = 'tunnel-pilot';

  String currentVersion = '';

  String? latestVersion;
  String? downloadUrl;
  String? releaseNotes;
  String? htmlUrl;

  bool isChecking = false;
  bool updateAvailable = false;
  bool isDownloading = false;
  bool isInstalling = false;
  double downloadProgress = 0.0;

  Timer? _periodicTimer;
  HttpClient? _httpClient;

  String? _lastSkippedVersion;

  HttpClient get _client {
    _httpClient ??= HttpClient()..connectionTimeout = const Duration(seconds: 10);
    return _httpClient!;
  }

  Future<void> init() async {
    final info = await PackageInfo.fromPlatform();
    currentVersion = info.version;
  }

  void setSkippedVersion(String? version) {
    _lastSkippedVersion = version;
  }

  Future<void> checkForUpdate() async {
    if (isChecking) return;

    if (currentVersion.isEmpty) {
      await init();
    }

    isChecking = true;
    notifyListeners();

    try {
      final request = await _client.getUrl(Uri.parse(
        'https://api.github.com/repos/$_repoOwner/$_repoName/releases/latest',
      ));
      request.headers.set('Accept', 'application/vnd.github.v3+json');
      request.headers.set('User-Agent', 'TunnelPilot/$currentVersion');

      final response = await request.close();

      if (response.statusCode != 200) {
        debugPrint('Update check failed: HTTP ${response.statusCode}');
        await response.drain();
        return;
      }

      final body = await response.transform(utf8.decoder).join();
      final json = jsonDecode(body) as Map<String, dynamic>;

      final tagName = json['tag_name'] as String? ?? '';
      final version = tagName.startsWith('v') ? tagName.substring(1) : tagName;

      if (version.isEmpty) return;

      // Skip pre-release
      if (json['prerelease'] == true) return;

      if (compareVersions(currentVersion, version) >= 0) {
        // Current version is up to date
        updateAvailable = false;
        notifyListeners();
        return;
      }

      // Skip if user chose to skip this version
      if (_lastSkippedVersion == version) {
        updateAvailable = false;
        notifyListeners();
        return;
      }

      latestVersion = version;
      releaseNotes = json['body'] as String?;
      htmlUrl = json['html_url'] as String?;

      // Find platform-specific asset
      final assets = json['assets'] as List<dynamic>? ?? [];
      downloadUrl = _findPlatformAsset(assets);

      updateAvailable = true;
      notifyListeners();
    } catch (e) {
      debugPrint('Update check error: $e');
    } finally {
      isChecking = false;
      notifyListeners();
    }
  }

  String? _findPlatformAsset(List<dynamic> assets) {
    String pattern;
    if (Platform.isMacOS) {
      pattern = '-macos.dmg';
    } else if (Platform.isWindows) {
      pattern = '-windows.zip';
    } else if (Platform.isLinux) {
      pattern = '-linux.tar.gz';
    } else {
      return null;
    }

    for (final asset in assets) {
      final name = asset['name'] as String? ?? '';
      if (name.endsWith(pattern)) {
        return asset['browser_download_url'] as String?;
      }
    }
    return null;
  }

  /// Compare two semver strings. Returns negative if a < b, 0 if equal, positive if a > b.
  @visibleForTesting
  int compareVersions(String a, String b) {
    final aParts = a.split('.').map((e) => int.tryParse(e) ?? 0).toList();
    final bParts = b.split('.').map((e) => int.tryParse(e) ?? 0).toList();

    final len = aParts.length > bParts.length ? aParts.length : bParts.length;
    for (int i = 0; i < len; i++) {
      final aVal = i < aParts.length ? aParts[i] : 0;
      final bVal = i < bParts.length ? bParts[i] : 0;
      if (aVal != bVal) return aVal - bVal;
    }
    return 0;
  }

  Future<void> downloadAndInstall() async {
    if (downloadUrl == null || isDownloading) return;

    isDownloading = true;
    downloadProgress = 0.0;
    notifyListeners();

    try {
      final request = await _client.getUrl(Uri.parse(downloadUrl!));
      final response = await request.close();

      final contentLength = response.contentLength;
      final tempDir = await getTemporaryDirectory();
      final fileName = downloadUrl!.split('/').last;
      final filePath = '${tempDir.path}/$fileName';

      final file = File(filePath);
      final sink = file.openWrite();
      int received = 0;
      int lastNotifiedPercent = -1;

      await for (final chunk in response) {
        sink.add(chunk);
        received += chunk.length;
        if (contentLength > 0) {
          downloadProgress = received / contentLength;
          // Throttle UI updates to every 2% to avoid excessive rebuilds
          final percent = (downloadProgress * 50).floor(); // 50 steps = 2% each
          if (percent != lastNotifiedPercent) {
            lastNotifiedPercent = percent;
            notifyListeners();
          }
        }
      }

      await sink.flush();
      await sink.close();

      downloadProgress = 1.0;
      isDownloading = false;
      isInstalling = true;
      notifyListeners();

      // Install and restart, or fallback to opening the file
      if (Platform.isMacOS && filePath.endsWith('.dmg')) {
        await _installAndRestartMacOS(filePath);
      } else if (Platform.isWindows && filePath.endsWith('.zip')) {
        await _installAndRestartWindows(filePath);
      } else if (Platform.isLinux && filePath.endsWith('.tar.gz')) {
        await _installAndRestartLinux(filePath);
      } else {
        await _openDownloadedFile(filePath);
      }
    } catch (e) {
      debugPrint('Download error: $e');
    } finally {
      isDownloading = false;
      isInstalling = false;
      downloadProgress = 0.0;
      notifyListeners();
    }
  }

  Future<void> _installAndRestartMacOS(String filePath) async {
    try {
      // Mount the DMG
      final mountResult = await Process.run('hdiutil', ['attach', filePath, '-nobrowse']);
      if (mountResult.exitCode != 0) {
        debugPrint('Failed to mount DMG: ${mountResult.stderr}');
        await _openDownloadedFile(filePath);
        return;
      }

      // Find the mount point from hdiutil output
      final output = mountResult.stdout as String;
      final lines = output.trim().split('\n');
      String? mountPoint;
      for (final line in lines) {
        // hdiutil output: /dev/diskX  ... /Volumes/AppName
        final match = RegExp(r'\t(/Volumes/.+)$').firstMatch(line);
        if (match != null) {
          mountPoint = match.group(1)?.trim();
          break;
        }
      }

      if (mountPoint == null) {
        debugPrint('Could not find mount point');
        await _openDownloadedFile(filePath);
        return;
      }

      // Find the .app in the mounted DMG
      final mountDir = Directory(mountPoint);
      final apps = mountDir
          .listSync()
          .whereType<Directory>()
          .where((d) => d.path.endsWith('.app'))
          .toList();

      if (apps.isEmpty) {
        debugPrint('No .app found in DMG');
        await Process.run('hdiutil', ['detach', mountPoint]);
        await _openDownloadedFile(filePath);
        return;
      }

      final appSource = apps.first.path;
      final appName = appSource.split('/').last;
      final appDest = '/Applications/$appName';

      // Remove old app and copy new one
      if (Directory(appDest).existsSync()) {
        await Process.run('rm', ['-rf', appDest]);
      }
      final copyResult = await Process.run('cp', ['-R', appSource, appDest]);
      if (copyResult.exitCode != 0) {
        debugPrint('Failed to copy app: ${copyResult.stderr}');
        await Process.run('hdiutil', ['detach', mountPoint]);
        await _openDownloadedFile(filePath);
        return;
      }

      // Detach the DMG
      await Process.run('hdiutil', ['detach', mountPoint]);

      // Relaunch the app
      await Process.run('open', ['-n', appDest]);
      exit(0);
    } catch (e) {
      debugPrint('Auto-install failed: $e');
      await _openDownloadedFile(filePath);
    }
  }

  Future<void> _installAndRestartWindows(String filePath) async {
    try {
      final appDir = File(Platform.resolvedExecutable).parent.path;
      final tempExtractDir = '${File(filePath).parent.path}\\tunnel_pilot_update';

      // Clean up previous extract if exists
      if (Directory(tempExtractDir).existsSync()) {
        Directory(tempExtractDir).deleteSync(recursive: true);
      }

      // Extract the zip using PowerShell
      final extractResult = await Process.run('powershell', [
        '-Command',
        'Expand-Archive -Path "$filePath" -DestinationPath "$tempExtractDir" -Force',
      ]);
      if (extractResult.exitCode != 0) {
        debugPrint('Failed to extract zip: ${extractResult.stderr}');
        await _openDownloadedFile(filePath);
        return;
      }

      // Use a batch script to replace files and restart
      // The script waits for the current process to exit, then copies files
      final scriptPath = '${File(filePath).parent.path}\\update.bat';
      final script = '''
@echo off
timeout /t 2 /nobreak >nul
xcopy /s /y /q "$tempExtractDir\\*" "$appDir\\"
start "" "$appDir\\tunnel_pilot.exe"
del "%~f0"
''';
      File(scriptPath).writeAsStringSync(script);

      await Process.start('cmd', ['/c', scriptPath],
          mode: ProcessStartMode.detached);
      exit(0);
    } catch (e) {
      debugPrint('Auto-install failed (Windows): $e');
      await _openDownloadedFile(filePath);
    }
  }

  Future<void> _installAndRestartLinux(String filePath) async {
    try {
      final appDir = File(Platform.resolvedExecutable).parent.parent.path;
      final tempExtractDir = '${File(filePath).parent.path}/tunnel_pilot_update';

      // Clean up previous extract if exists
      if (Directory(tempExtractDir).existsSync()) {
        Directory(tempExtractDir).deleteSync(recursive: true);
      }
      Directory(tempExtractDir).createSync();

      // Extract the tar.gz
      final extractResult = await Process.run('tar', [
        'xzf', filePath, '-C', tempExtractDir,
      ]);
      if (extractResult.exitCode != 0) {
        debugPrint('Failed to extract tar.gz: ${extractResult.stderr}');
        await _openDownloadedFile(filePath);
        return;
      }

      // Use a shell script to replace files and restart
      final scriptPath = '${File(filePath).parent.path}/update.sh';
      final script = '''
#!/bin/bash
sleep 2
cp -rf "$tempExtractDir"/* "$appDir/"
"$appDir/tunnel_pilot" &
rm -f "\$0"
''';
      File(scriptPath).writeAsStringSync(script);
      await Process.run('chmod', ['+x', scriptPath]);

      await Process.start('bash', [scriptPath],
          mode: ProcessStartMode.detached);
      exit(0);
    } catch (e) {
      debugPrint('Auto-install failed (Linux): $e');
      await _openDownloadedFile(filePath);
    }
  }

  Future<void> _openDownloadedFile(String filePath) async {
    if (Platform.isMacOS) {
      await Process.run('open', [filePath]);
    } else if (Platform.isWindows) {
      // Open Explorer with the file selected
      await Process.run('explorer.exe', ['/select,', filePath]);
    } else if (Platform.isLinux) {
      // Open the containing folder
      final dir = File(filePath).parent.path;
      await Process.run('xdg-open', [dir]);
    }
  }

  Future<void> openReleasePage() async {
    final url = htmlUrl;
    if (url == null) return;

    if (Platform.isMacOS) {
      await Process.run('open', [url]);
    } else if (Platform.isWindows) {
      await Process.run('cmd', ['/c', 'start', '', url]);
    } else if (Platform.isLinux) {
      await Process.run('xdg-open', [url]);
    }
  }

  void startPeriodicCheck({Duration interval = const Duration(hours: 6)}) {
    _periodicTimer?.cancel();
    _periodicTimer = Timer.periodic(interval, (_) => checkForUpdate());
  }

  void stopPeriodicCheck() {
    _periodicTimer?.cancel();
    _periodicTimer = null;
  }

  void dismissUpdate() {
    updateAvailable = false;
    notifyListeners();
  }

  @override
  void dispose() {
    _periodicTimer?.cancel();
    _httpClient?.close();
    _httpClient = null;
    super.dispose();
  }
}
