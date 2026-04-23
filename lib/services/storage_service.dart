import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../models/app_settings.dart';
import '../models/forward_config.dart';

class BackupImportException implements Exception {
  final String message;
  BackupImportException(this.message);
  @override
  String toString() => message;
}

class StorageService {
  static const _configFileName = 'tunnel_pilot_config.json';
  static const _currentBackupVersion = 1;

  File? _configFile;

  Future<File> get configFile async {
    if (_configFile != null) return _configFile!;
    final dir = await getApplicationSupportDirectory();
    _configFile = File('${dir.path}/$_configFileName');
    return _configFile!;
  }

  Future<({List<ForwardConfig> forwards, AppSettings settings})> load() async {
    final file = await configFile;
    if (!await file.exists()) {
      return (forwards: <ForwardConfig>[], settings: AppSettings());
    }

    final content = await file.readAsString();
    if (content.isEmpty) {
      return (forwards: <ForwardConfig>[], settings: AppSettings());
    }

    final json = jsonDecode(content) as Map<String, dynamic>;

    final forwards = (json['forwards'] as List<dynamic>?)
            ?.map((e) => ForwardConfig.fromJson(e as Map<String, dynamic>))
            .toList() ??
        [];

    final settings = json['settings'] != null
        ? AppSettings.fromJson(json['settings'] as Map<String, dynamic>)
        : AppSettings();

    return (forwards: forwards, settings: settings);
  }

  Future<void> saveForwards(List<ForwardConfig> forwards) async {
    final file = await configFile;
    Map<String, dynamic> json = {};

    if (await file.exists()) {
      final content = await file.readAsString();
      if (content.isNotEmpty) {
        json = jsonDecode(content) as Map<String, dynamic>;
      }
    }

    json['forwards'] = forwards.map((f) => f.toJson()).toList();
    await file.writeAsString(const JsonEncoder.withIndent('  ').convert(json));
  }

  Future<void> saveSettings(AppSettings settings) async {
    final file = await configFile;
    Map<String, dynamic> json = {};

    if (await file.exists()) {
      final content = await file.readAsString();
      if (content.isNotEmpty) {
        json = jsonDecode(content) as Map<String, dynamic>;
      }
    }

    json['settings'] = settings.toJson();
    await file.writeAsString(const JsonEncoder.withIndent('  ').convert(json));
  }

  Future<void> exportToFile(String path, List<ForwardConfig> forwards) async {
    final backup = {
      'version': _currentBackupVersion,
      'exportedAt': DateTime.now().toIso8601String(),
      'forwards': forwards.map((f) => f.toJsonForBackup()).toList(),
    };
    final file = File(path);
    await file.writeAsString(const JsonEncoder.withIndent('  ').convert(backup));
  }

  Future<List<ForwardConfig>> importFromFile(String path) async {
    final file = File(path);
    if (!await file.exists()) {
      throw BackupImportException('Backup file not found: $path');
    }

    final content = await file.readAsString();

    Map<String, dynamic> json;
    try {
      final decoded = jsonDecode(content);
      if (decoded is! Map<String, dynamic>) {
        throw BackupImportException(
            'Invalid backup: root is not a JSON object');
      }
      json = decoded;
    } on FormatException catch (e) {
      throw BackupImportException('Invalid backup: malformed JSON (${e.message})');
    }

    final version = json['version'];
    if (version is int && version > _currentBackupVersion) {
      throw BackupImportException(
          'Backup version $version is newer than this app supports '
          '(max v$_currentBackupVersion). Please update Tunnel Pilot.');
    }

    final rawForwards = json['forwards'];
    if (rawForwards is! List) {
      throw BackupImportException(
          'Invalid backup: missing or invalid "forwards" field');
    }

    final result = <ForwardConfig>[];
    for (var i = 0; i < rawForwards.length; i++) {
      final entry = rawForwards[i];
      if (entry is! Map<String, dynamic>) {
        throw BackupImportException(
            'Invalid backup: entry #${i + 1} is not an object');
      }
      try {
        result.add(ForwardConfig.fromJson(entry));
      } catch (e) {
        throw BackupImportException(
            'Invalid backup: entry #${i + 1} is malformed ($e)');
      }
    }

    return result;
  }
}
