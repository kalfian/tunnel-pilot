import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';

class UpdateService extends ChangeNotifier {
  static const String _repoOwner = 'kalfian';
  static const String _repoName = 'tunnel-pilot';
  static const Duration _downloadIdleTimeout = Duration(seconds: 60);

  String _currentVersion = '';
  String? _latestVersion;
  String? _downloadUrl;
  String? _releaseNotes;
  String? _htmlUrl;

  bool _isChecking = false;
  bool _updateAvailable = false;
  bool _isUpToDate = false;
  bool _isDownloading = false;
  bool _isInstalling = false;
  bool _readyToInstall = false;
  double _downloadProgress = 0.0;
  int _downloadedBytes = 0;
  int _totalBytes = 0;

  String? _downloadedFilePath;
  String? _errorMessage;
  String? _checkError;
  String? _statusMessage;
  bool _cancelRequested = false;

  Timer? _periodicTimer;
  HttpClient? _httpClient;
  HttpClient? _downloadClient;

  String? _lastSkippedVersion;

  String get currentVersion => _currentVersion;
  String? get latestVersion => _latestVersion;
  String? get downloadUrl => _downloadUrl;
  String? get releaseNotes => _releaseNotes;
  String? get htmlUrl => _htmlUrl;
  bool get isChecking => _isChecking;
  bool get updateAvailable => _updateAvailable;
  bool get isUpToDate => _isUpToDate;
  bool get isDownloading => _isDownloading;
  bool get isInstalling => _isInstalling;
  bool get readyToInstall => _readyToInstall;
  double get downloadProgress => _downloadProgress;
  int get downloadedBytes => _downloadedBytes;
  int get totalBytes => _totalBytes;
  String? get errorMessage => _errorMessage;
  String? get checkError => _checkError;
  String? get statusMessage => _statusMessage;

  @visibleForTesting
  set updateAvailable(bool v) => _updateAvailable = v;
  @visibleForTesting
  set isDownloading(bool v) => _isDownloading = v;
  @visibleForTesting
  set isInstalling(bool v) => _isInstalling = v;
  @visibleForTesting
  set readyToInstall(bool v) => _readyToInstall = v;
  @visibleForTesting
  set downloadProgress(double v) => _downloadProgress = v;
  @visibleForTesting
  set errorMessage(String? v) => _errorMessage = v;
  @visibleForTesting
  set statusMessage(String? v) => _statusMessage = v;
  @visibleForTesting
  set downloadUrl(String? v) => _downloadUrl = v;
  @visibleForTesting
  set latestVersion(String? v) => _latestVersion = v;

  HttpClient get _client {
    _httpClient ??= HttpClient()
      ..connectionTimeout = const Duration(seconds: 10);
    return _httpClient!;
  }

  Future<void> init() async {
    final info = await PackageInfo.fromPlatform();
    _currentVersion = info.version;
  }

  void setSkippedVersion(String? version) {
    _lastSkippedVersion = version;
  }

  void cancelUpdate() {
    _cancelRequested = true;
    try {
      _downloadClient?.close(force: true);
    } catch (_) {}
    _downloadClient = null;
    _cleanupDownloadedFile();
    _isDownloading = false;
    _isInstalling = false;
    _readyToInstall = false;
    _downloadedFilePath = null;
    _downloadProgress = 0.0;
    _downloadedBytes = 0;
    _totalBytes = 0;
    _statusMessage = null;
    _errorMessage = null;
    notifyListeners();
  }

  void _cleanupDownloadedFile() {
    if (_downloadedFilePath != null) {
      try {
        File(_downloadedFilePath!).deleteSync();
      } catch (_) {}
      _downloadedFilePath = null;
    }
  }

  Future<void> checkForUpdate() async {
    if (_isChecking) return;

    if (_currentVersion.isEmpty) {
      await init();
    }

    _isChecking = true;
    _checkError = null;
    _isUpToDate = false;
    notifyListeners();

    try {
      final request = await _client.getUrl(Uri.parse(
        'https://api.github.com/repos/$_repoOwner/$_repoName/releases/latest',
      ));
      request.headers.set('Accept', 'application/vnd.github.v3+json');
      request.headers.set('User-Agent', 'TunnelPilot/$_currentVersion');

      final response = await request.close();

      if (response.statusCode == 403 || response.statusCode == 429) {
        final resetHeader = response.headers.value('x-ratelimit-reset');
        await response.drain();
        if (resetHeader != null) {
          final resetTime = DateTime.fromMillisecondsSinceEpoch(
            int.parse(resetHeader) * 1000,
          );
          final waitMinutes =
              resetTime.difference(DateTime.now()).inMinutes.clamp(1, 60);
          _checkError = 'GitHub rate limited. Try again in ${waitMinutes}m.';
        } else {
          _checkError = 'GitHub rate limited. Try again later.';
        }
        notifyListeners();
        return;
      }

      if (response.statusCode != 200) {
        await response.drain();
        _checkError = 'Check failed (HTTP ${response.statusCode})';
        notifyListeners();
        return;
      }

      final body = await response.transform(utf8.decoder).join();
      final json = jsonDecode(body) as Map<String, dynamic>;

      final tagName = json['tag_name'] as String? ?? '';
      final version =
          tagName.startsWith('v') ? tagName.substring(1) : tagName;

      if (version.isEmpty) {
        _checkError = 'Could not parse version from release.';
        notifyListeners();
        return;
      }

      if (json['prerelease'] == true) {
        _isUpToDate = true;
        _updateAvailable = false;
        notifyListeners();
        return;
      }

      if (compareVersions(_currentVersion, version) >= 0) {
        _isUpToDate = true;
        _updateAvailable = false;
        notifyListeners();
        return;
      }

      if (_lastSkippedVersion == version) {
        _updateAvailable = false;
        notifyListeners();
        return;
      }

      _latestVersion = version;
      _releaseNotes = json['body'] as String?;
      _htmlUrl = json['html_url'] as String?;

      final assets = json['assets'] as List<dynamic>? ?? [];
      _downloadUrl = _findPlatformAsset(assets);

      _updateAvailable = true;
      notifyListeners();
    } catch (e) {
      debugPrint('Update check error: $e');
      _checkError = 'Network error. Check your connection.';
      notifyListeners();
    } finally {
      _isChecking = false;
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

  @visibleForTesting
  int compareVersions(String a, String b) {
    final aNoBuild = a.split('+').first;
    final bNoBuild = b.split('+').first;

    final aDash = aNoBuild.indexOf('-');
    final bDash = bNoBuild.indexOf('-');

    final aCore = aDash == -1 ? aNoBuild : aNoBuild.substring(0, aDash);
    final bCore = bDash == -1 ? bNoBuild : bNoBuild.substring(0, bDash);
    final aPre = aDash == -1 ? '' : aNoBuild.substring(aDash + 1);
    final bPre = bDash == -1 ? '' : bNoBuild.substring(bDash + 1);

    final aParts = aCore.split('.').map((e) => int.tryParse(e) ?? 0).toList();
    final bParts = bCore.split('.').map((e) => int.tryParse(e) ?? 0).toList();

    final len =
        aParts.length > bParts.length ? aParts.length : bParts.length;
    for (int i = 0; i < len; i++) {
      final aVal = i < aParts.length ? aParts[i] : 0;
      final bVal = i < bParts.length ? bParts[i] : 0;
      if (aVal != bVal) return aVal - bVal;
    }

    if (aPre.isEmpty && bPre.isEmpty) return 0;
    if (aPre.isEmpty) return 1;
    if (bPre.isEmpty) return -1;
    return aPre.compareTo(bPre);
  }

  Future<void> downloadAndInstall() async {
    if (_downloadUrl == null || _isDownloading) return;

    _cancelRequested = false;
    _errorMessage = null;
    _statusMessage = null;
    _isDownloading = true;
    _downloadProgress = 0.0;
    _downloadedBytes = 0;
    _totalBytes = 0;
    notifyListeners();

    _downloadClient = HttpClient()
      ..connectionTimeout = const Duration(seconds: 15);

    HttpClientResponse? response;
    IOSink? sink;
    File? partialFile;

    try {
      final request = await _downloadClient!.getUrl(Uri.parse(_downloadUrl!));
      response = await request.close();
      if (response.statusCode < 200 || response.statusCode >= 300) {
        await response.drain<void>();
        _errorMessage = 'Download failed (HTTP ${response.statusCode}).';
        return;
      }

      final contentLength = response.contentLength;
      _totalBytes = contentLength > 0 ? contentLength : 0;
      final tempDir = await getTemporaryDirectory();
      final fileName = _downloadUrl!.split('/').last;
      final filePath = '${tempDir.path}/$fileName';

      partialFile = File(filePath);
      sink = partialFile.openWrite();
      int received = 0;
      int lastNotifiedPercent = -1;

      await for (final chunk in response.timeout(_downloadIdleTimeout)) {
        if (_cancelRequested) return;
        var bytesToWrite = chunk.length;
        if (contentLength > 0) {
          final remaining = contentLength - received;
          if (remaining <= 0) {
            break;
          }
          if (bytesToWrite > remaining) {
            bytesToWrite = remaining;
          }
        }
        if (bytesToWrite > 0) {
          sink.add(chunk.sublist(0, bytesToWrite));
          received += bytesToWrite;
        }
        _downloadedBytes = received;
        if (contentLength > 0) {
          _downloadProgress = received / contentLength;
          final percent = (_downloadProgress * 100).floor();
          if (percent != lastNotifiedPercent) {
            lastNotifiedPercent = percent;
            notifyListeners();
          }
          if (received >= contentLength) {
            break;
          }
        }
      }

      if (_cancelRequested) return;

      await sink.flush();
      await sink.close();
      sink = null;
      response = null;

      if (contentLength > 0) {
        final fileSize = await partialFile.length();
        if (fileSize != contentLength) {
          _errorMessage =
              'Download corrupted (${_formatBytes(fileSize)} of ${_formatBytes(contentLength)}). Try again.';
          return;
        }
      }

      partialFile = null;

      _downloadProgress = 1.0;
      _isDownloading = false;
      _readyToInstall = true;
      _downloadedFilePath = filePath;
      notifyListeners();
    } catch (e) {
      if (_cancelRequested) return;
      debugPrint('Download error: $e');
      _errorMessage = e is TimeoutException
          ? 'Download stalled. Please try again.'
          : 'Download failed. Please try again.';
    } finally {
      if (sink != null) {
        try {
          await sink.close();
        } catch (_) {}
      }
      if (response != null) {
        try {
          await response.drain<void>();
        } catch (_) {}
      }
      if (partialFile != null) {
        try {
          await partialFile.delete();
        } catch (_) {}
      }
      try {
        _downloadClient?.close();
      } catch (_) {}
      _downloadClient = null;

      if (!_readyToInstall) {
        _isDownloading = false;
        _downloadProgress = 0.0;
        _downloadedBytes = 0;
        _totalBytes = 0;
        _statusMessage = null;
        notifyListeners();
      }
    }
  }

  Future<void> installManually() async {
    try {
      final filePath = _downloadedFilePath;
      if (filePath != null) {
        await _openDownloadedFile(filePath);
        return;
      }
      await openReleasePage();
    } catch (_) {
      _errorMessage =
          'Could not open installer automatically. Please open the GitHub releases page to install manually.';
      notifyListeners();
    }
  }

  Future<void> proceedWithInstall() async {
    if (_downloadedFilePath == null) return;

    _readyToInstall = false;
    _isInstalling = true;
    _statusMessage = 'Preparing update...';
    notifyListeners();

    try {
      final filePath = _downloadedFilePath!;
      if (Platform.isMacOS && filePath.endsWith('.dmg')) {
        await _installDetachedMacOS(filePath);
      } else if (Platform.isWindows && filePath.endsWith('.zip')) {
        await _installDetachedWindows(filePath);
      } else if (Platform.isLinux && filePath.endsWith('.tar.gz')) {
        await _installDetachedLinux(filePath);
      } else {
        await _openDownloadedFile(filePath);
        _isInstalling = false;
        _statusMessage = null;
        notifyListeners();
      }
    } catch (e) {
      debugPrint('Install failed: $e');
      _errorMessage =
          'Install failed. Please try again or install manually.';
      _isInstalling = false;
      _downloadedFilePath = null;
      _statusMessage = null;
      notifyListeners();
    }
  }

  Future<void> _installDetachedMacOS(String dmgPath) async {
    final currentPid = pid;
    final tempDir = await getTemporaryDirectory();
    final scriptPath = '${tempDir.path}/tunnel_pilot_update.sh';

    final script = '''#!/bin/bash
while kill -0 $currentPid 2>/dev/null; do sleep 0.5; done
sleep 1

MOUNT_OUTPUT=\$(hdiutil attach "$dmgPath" -nobrowse 2>&1)
MOUNT_POINT=\$(echo "\$MOUNT_OUTPUT" | sed -n 's/.*\\(\\/Volumes\\/.*\\)/\\1/p' | sed 's/[[:space:]]*\$//' | head -1)

if [ -z "\$MOUNT_POINT" ]; then
  open "$dmgPath"
  rm -f "\$0"
  exit 1
fi

APP_SOURCE=\$(find "\$MOUNT_POINT" -maxdepth 1 -name "*.app" -print -quit)
if [ -z "\$APP_SOURCE" ]; then
  hdiutil detach "\$MOUNT_POINT" -quiet 2>/dev/null
  open "$dmgPath"
  rm -f "\$0"
  exit 1
fi

APP_NAME=\$(basename "\$APP_SOURCE")
APP_DEST="/Applications/\$APP_NAME"
APP_BAK="/Applications/.\$APP_NAME.bak"

rm -rf "\$APP_BAK"
if [ -d "\$APP_DEST" ]; then
  mv "\$APP_DEST" "\$APP_BAK"
fi

if cp -R "\$APP_SOURCE" "\$APP_DEST"; then
  rm -rf "\$APP_BAK"
else
  if [ -d "\$APP_BAK" ]; then
    mv "\$APP_BAK" "\$APP_DEST"
  fi
fi

hdiutil detach "\$MOUNT_POINT" -quiet 2>/dev/null
rm -f "$dmgPath"
open "\$APP_DEST"
rm -f "\$0"
''';

    File(scriptPath).writeAsStringSync(script);
    await Process.run('chmod', ['+x', scriptPath]);

    _statusMessage = 'Closing app to install update...';
    notifyListeners();
    await Future.delayed(const Duration(milliseconds: 500));

    await Process.start('bash', [scriptPath],
        mode: ProcessStartMode.detached);
    exit(0);
  }

  Future<void> _installDetachedWindows(String zipPath) async {
    final currentPid = pid;
    final appDir = File(Platform.resolvedExecutable).parent.path;
    final tempDir = File(zipPath).parent.path;
    final extractDir = '$tempDir\\tunnel_pilot_update';
    final scriptPath = '$tempDir\\tunnel_pilot_update.bat';

    final script = '''@echo off
:wait
tasklist /FI "PID eq $currentPid" 2>nul | find "$currentPid" >nul
if not errorlevel 1 (
  timeout /t 1 /nobreak >nul
  goto wait
)
timeout /t 1 /nobreak >nul
powershell -Command "Expand-Archive -Path '$zipPath' -DestinationPath '$extractDir' -Force"
xcopy /s /y /q "$extractDir\\*" "$appDir\\"
start "" "$appDir\\tunnel_pilot.exe"
rmdir /s /q "$extractDir"
del "$zipPath"
del "%~f0"
''';

    File(scriptPath).writeAsStringSync(script);

    _statusMessage = 'Closing app to install update...';
    notifyListeners();
    await Future.delayed(const Duration(milliseconds: 500));

    await Process.start('cmd', ['/c', scriptPath],
        mode: ProcessStartMode.detached);
    exit(0);
  }

  Future<void> _installDetachedLinux(String tarPath) async {
    final currentPid = pid;
    final appDir = File(Platform.resolvedExecutable).parent.parent.path;
    final tempDir = File(tarPath).parent.path;
    final extractDir = '$tempDir/tunnel_pilot_update';
    final scriptPath = '$tempDir/tunnel_pilot_update.sh';

    final script = '''#!/bin/bash
while kill -0 $currentPid 2>/dev/null; do sleep 0.5; done
sleep 1
mkdir -p "$extractDir"
tar xzf "$tarPath" -C "$extractDir"
cp -rf "$extractDir"/* "$appDir/"
rm -rf "$extractDir"
rm -f "$tarPath"
"$appDir/tunnel_pilot" &
rm -f "\$0"
''';

    File(scriptPath).writeAsStringSync(script);
    await Process.run('chmod', ['+x', scriptPath]);

    _statusMessage = 'Closing app to install update...';
    notifyListeners();
    await Future.delayed(const Duration(milliseconds: 500));

    await Process.start('bash', [scriptPath],
        mode: ProcessStartMode.detached);
    exit(0);
  }

  Future<void> _openDownloadedFile(String filePath) async {
    if (Platform.isMacOS) {
      await Process.run('open', [filePath]);
    } else if (Platform.isWindows) {
      await Process.run('explorer.exe', ['/select,', filePath]);
    } else if (Platform.isLinux) {
      final dir = File(filePath).parent.path;
      await Process.run('xdg-open', [dir]);
    }
  }

  Future<void> openReleasePage() async {
    final url = _htmlUrl;
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
    _cleanupDownloadedFile();
    _updateAvailable = false;
    _readyToInstall = false;
    _errorMessage = null;
    _statusMessage = null;
    notifyListeners();
  }

  void clearCheckStatus() {
    _isUpToDate = false;
    _checkError = null;
    notifyListeners();
  }

  static String _formatBytes(int bytes) {
    if (bytes < 1024) return '$bytes B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
    if (bytes < 1024 * 1024 * 1024) {
      return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
    }
    return '${(bytes / (1024 * 1024 * 1024)).toStringAsFixed(1)} GB';
  }

  String get downloadSizeText {
    if (_totalBytes <= 0) return '';
    return '${_formatBytes(_downloadedBytes)} / ${_formatBytes(_totalBytes)}';
  }

  @override
  void dispose() {
    _periodicTimer?.cancel();
    _httpClient?.close();
    _httpClient = null;
    _downloadClient?.close();
    _downloadClient = null;
    super.dispose();
  }
}
