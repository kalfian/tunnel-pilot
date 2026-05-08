import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';

class UpdateService extends ChangeNotifier {
  static const String _repoOwner = 'kalfian';
  static const String _repoName = 'tunnel-pilot';

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
  double _downloadProgress = 0.0;
  int _downloadedBytes = 0;
  int _totalBytes = 0;

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
    _isDownloading = false;
    _isInstalling = false;
    _downloadProgress = 0.0;
    _downloadedBytes = 0;
    _totalBytes = 0;
    _statusMessage = null;
    _errorMessage = null;
    notifyListeners();
  }

  Future<ProcessResult> _runWithTimeout(
    String executable,
    List<String> arguments, {
    Duration timeout = const Duration(seconds: 60),
  }) {
    return Process.run(executable, arguments).timeout(
      timeout,
      onTimeout: () => throw TimeoutException(
        '$executable timed out after ${timeout.inSeconds}s',
      ),
    );
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

      final contentLength = response.contentLength;
      _totalBytes = contentLength > 0 ? contentLength : 0;
      final tempDir = await getTemporaryDirectory();
      final fileName = _downloadUrl!.split('/').last;
      final filePath = '${tempDir.path}/$fileName';

      partialFile = File(filePath);
      sink = partialFile.openWrite();
      int received = 0;
      int lastNotifiedPercent = -1;

      await for (final chunk in response) {
        if (_cancelRequested) return;
        sink.add(chunk);
        received += chunk.length;
        _downloadedBytes = received;
        if (contentLength > 0) {
          _downloadProgress = received / contentLength;
          final percent = (_downloadProgress * 50).floor();
          if (percent != lastNotifiedPercent) {
            lastNotifiedPercent = percent;
            notifyListeners();
          }
        }
      }

      if (_cancelRequested) return;

      await sink.flush();
      await sink.close();
      sink = null;
      response = null;

      // Verify file integrity — size must match Content-Length
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
      _isInstalling = true;
      notifyListeners();

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
      if (_cancelRequested) return;
      debugPrint('Download/install error: $e');
      _errorMessage = e is TimeoutException
          ? 'Update timed out. Please try again or download manually.'
          : 'Update failed. Please try again or download manually.';
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

      _isDownloading = false;
      _isInstalling = false;
      _downloadProgress = 0.0;
      _downloadedBytes = 0;
      _totalBytes = 0;
      _statusMessage = null;
      notifyListeners();
    }
  }

  Future<void> _installAndRestartMacOS(String filePath) async {
    String? mountPoint;
    try {
      _statusMessage = 'Mounting disk image...';
      notifyListeners();
      final mountResult = await _runWithTimeout(
        'hdiutil',
        ['attach', filePath, '-nobrowse'],
        timeout: const Duration(seconds: 60),
      );
      if (mountResult.exitCode != 0) {
        debugPrint('Failed to mount DMG: ${mountResult.stderr}');
        _errorMessage =
            'Auto-install failed. The file has been opened — please install manually.';
        await _openDownloadedFile(filePath);
        return;
      }

      final output = mountResult.stdout as String;
      final lines = output.trim().split('\n');
      for (final line in lines) {
        final match = RegExp(r'\s(/Volumes/.+)$').firstMatch(line);
        if (match != null) {
          mountPoint = match.group(1)?.trim();
          break;
        }
      }

      if (mountPoint == null) {
        debugPrint('Could not find mount point');
        _errorMessage =
            'Auto-install failed. The file has been opened — please install manually.';
        await _openDownloadedFile(filePath);
        return;
      }

      final mountDir = Directory(mountPoint);
      final apps = mountDir
          .listSync()
          .whereType<Directory>()
          .where((d) => d.path.endsWith('.app'))
          .toList();

      if (apps.isEmpty) {
        debugPrint('No .app found in DMG');
        await _runWithTimeout('hdiutil', ['detach', mountPoint],
            timeout: const Duration(seconds: 30));
        _errorMessage =
            'Auto-install failed. The file has been opened — please install manually.';
        await _openDownloadedFile(filePath);
        return;
      }

      final appSource = apps.first.path;
      final appName = appSource.split('/').last;
      final appDest = '/Applications/$appName';
      final appBackup = '/Applications/.$appName.bak';

      _statusMessage = 'Installing update...';
      notifyListeners();

      // Safe install: backup old → copy new → remove backup
      // If copy fails, old app is still available in backup
      if (Directory(appDest).existsSync()) {
        if (Directory(appBackup).existsSync()) {
          await _runWithTimeout('rm', ['-rf', appBackup],
              timeout: const Duration(seconds: 30));
        }
        final mvResult = await _runWithTimeout('mv', [appDest, appBackup],
            timeout: const Duration(seconds: 30));
        if (mvResult.exitCode != 0) {
          debugPrint('Failed to backup old app: ${mvResult.stderr}');
          await _runWithTimeout('hdiutil', ['detach', mountPoint],
              timeout: const Duration(seconds: 30));
          _errorMessage =
              'Auto-install failed. The file has been opened — please install manually.';
          await _openDownloadedFile(filePath);
          return;
        }
      }

      final copyResult = await _runWithTimeout(
        'cp',
        ['-R', appSource, appDest],
        timeout: const Duration(seconds: 120),
      );
      if (copyResult.exitCode != 0) {
        debugPrint('Failed to copy app: ${copyResult.stderr}');
        // Restore backup
        if (Directory(appBackup).existsSync()) {
          await _runWithTimeout('mv', [appBackup, appDest],
              timeout: const Duration(seconds: 30));
        }
        await _runWithTimeout('hdiutil', ['detach', mountPoint],
            timeout: const Duration(seconds: 30));
        _errorMessage =
            'Auto-install failed. The file has been opened — please install manually.';
        await _openDownloadedFile(filePath);
        return;
      }

      // Copy succeeded — remove backup
      if (Directory(appBackup).existsSync()) {
        await _runWithTimeout('rm', ['-rf', appBackup],
            timeout: const Duration(seconds: 30));
      }

      _statusMessage = 'Cleaning up...';
      notifyListeners();
      await _runWithTimeout('hdiutil', ['detach', mountPoint],
          timeout: const Duration(seconds: 30));

      _statusMessage = 'Restarting app...';
      notifyListeners();
      await Process.run('open', ['-n', appDest]);
      exit(0);
    } catch (e) {
      debugPrint('Auto-install failed: $e');
      if (mountPoint != null) {
        try {
          await Process.run('hdiutil', ['detach', mountPoint]);
        } catch (_) {}
      }
      _errorMessage =
          'Auto-install failed. The file has been opened — please install manually.';
      await _openDownloadedFile(filePath);
    }
  }

  Future<void> _installAndRestartWindows(String filePath) async {
    try {
      final appDir = File(Platform.resolvedExecutable).parent.path;
      final tempExtractDir =
          '${File(filePath).parent.path}\\tunnel_pilot_update';

      if (Directory(tempExtractDir).existsSync()) {
        Directory(tempExtractDir).deleteSync(recursive: true);
      }

      _statusMessage = 'Extracting update...';
      notifyListeners();
      final extractResult = await _runWithTimeout(
        'powershell',
        [
          '-Command',
          'Expand-Archive -Path "$filePath" -DestinationPath "$tempExtractDir" -Force',
        ],
        timeout: const Duration(seconds: 120),
      );
      if (extractResult.exitCode != 0) {
        debugPrint('Failed to extract zip: ${extractResult.stderr}');
        _errorMessage =
            'Auto-install failed. The file has been opened — please install manually.';
        await _openDownloadedFile(filePath);
        return;
      }

      _statusMessage = 'Applying update...';
      notifyListeners();
      final scriptPath = '${File(filePath).parent.path}\\update.bat';
      final script = '''
@echo off
timeout /t 2 /nobreak >nul
xcopy /s /y /q "$tempExtractDir\\*" "$appDir\\"
start "" "$appDir\\tunnel_pilot.exe"
del "%~f0"
''';
      File(scriptPath).writeAsStringSync(script);

      _statusMessage = 'Restarting app...';
      notifyListeners();
      await Process.start('cmd', ['/c', scriptPath],
          mode: ProcessStartMode.detached);
      exit(0);
    } catch (e) {
      debugPrint('Auto-install failed (Windows): $e');
      _errorMessage =
          'Auto-install failed. The file has been opened — please install manually.';
      await _openDownloadedFile(filePath);
    }
  }

  Future<void> _installAndRestartLinux(String filePath) async {
    try {
      final appDir = File(Platform.resolvedExecutable).parent.parent.path;
      final tempExtractDir =
          '${File(filePath).parent.path}/tunnel_pilot_update';

      if (Directory(tempExtractDir).existsSync()) {
        Directory(tempExtractDir).deleteSync(recursive: true);
      }
      Directory(tempExtractDir).createSync();

      _statusMessage = 'Extracting update...';
      notifyListeners();
      final extractResult = await _runWithTimeout(
        'tar',
        ['xzf', filePath, '-C', tempExtractDir],
        timeout: const Duration(seconds: 120),
      );
      if (extractResult.exitCode != 0) {
        debugPrint('Failed to extract tar.gz: ${extractResult.stderr}');
        _errorMessage =
            'Auto-install failed. The file has been opened — please install manually.';
        await _openDownloadedFile(filePath);
        return;
      }

      _statusMessage = 'Applying update...';
      notifyListeners();
      final scriptPath = '${File(filePath).parent.path}/update.sh';
      final script = '''
#!/bin/bash
sleep 2
cp -rf "$tempExtractDir"/* "$appDir/"
"$appDir/tunnel_pilot" &
rm -f "\$0"
''';
      File(scriptPath).writeAsStringSync(script);
      await _runWithTimeout('chmod', ['+x', scriptPath],
          timeout: const Duration(seconds: 10));

      _statusMessage = 'Restarting app...';
      notifyListeners();
      await Process.start('bash', [scriptPath],
          mode: ProcessStartMode.detached);
      exit(0);
    } catch (e) {
      debugPrint('Auto-install failed (Linux): $e');
      _errorMessage =
          'Auto-install failed. The file has been opened — please install manually.';
      await _openDownloadedFile(filePath);
    }
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
    _updateAvailable = false;
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
