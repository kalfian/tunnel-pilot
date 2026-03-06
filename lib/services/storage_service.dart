import 'dart:convert';
import 'dart:io';

import 'package:path_provider/path_provider.dart';

import '../models/app_settings.dart';
import '../models/forward_config.dart';

class StorageService {
  static const _configFileName = 'tunnel_pilot_config.json';

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
      'version': 1,
      'exportedAt': DateTime.now().toIso8601String(),
      'forwards': forwards.map((f) => f.toJsonForBackup()).toList(),
    };
    final file = File(path);
    await file.writeAsString(const JsonEncoder.withIndent('  ').convert(backup));
  }

  Future<List<ForwardConfig>> importFromFile(String path) async {
    final file = File(path);
    final content = await file.readAsString();
    final json = jsonDecode(content) as Map<String, dynamic>;

    final forwards = (json['forwards'] as List<dynamic>)
        .map((e) => ForwardConfig.fromJson(e as Map<String, dynamic>))
        .toList();

    return forwards;
  }
}
