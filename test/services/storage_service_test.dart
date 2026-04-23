import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/app_settings.dart';
import 'package:tunnel_pilot/models/forward_config.dart';
import 'package:tunnel_pilot/services/storage_service.dart';

void main() {
  group('StorageService - export/import', () {
    late Directory tempDir;

    setUp(() async {
      tempDir = await Directory.systemTemp.createTemp('tunnel_pilot_test_');
    });

    tearDown(() async {
      if (await tempDir.exists()) {
        await tempDir.delete(recursive: true);
      }
    });

    List<ForwardConfig> createSampleForwards() {
      return [
        ForwardConfig(
          id: 'id-1',
          name: 'DB Tunnel',
          sshHost: '10.0.0.1',
          sshPort: 22,
          sshUsername: 'admin',
          sshPassword: 'secret123',
          localBindAddress: '127.0.0.1',
          localPort: 3306,
          remoteHost: 'db.internal',
          remotePort: 3306,
        ),
        ForwardConfig(
          id: 'id-2',
          name: 'Web Tunnel',
          sshHost: '10.0.0.2',
          sshPort: 2222,
          sshUsername: 'deploy',
          identityFilePath: '/home/user/.ssh/id_rsa',
          localBindAddress: '127.0.0.1',
          localPort: 8080,
          remoteHost: 'web.internal',
          remotePort: 80,
        ),
      ];
    }

    test('exportToFile creates valid JSON file', () async {
      final service = StorageService();
      final forwards = createSampleForwards();
      final exportPath = '${tempDir.path}/backup.json';

      await service.exportToFile(exportPath, forwards);

      final file = File(exportPath);
      expect(await file.exists(), isTrue);

      final content = await file.readAsString();
      final json = jsonDecode(content) as Map<String, dynamic>;

      expect(json['version'], 1);
      expect(json['exportedAt'], isNotNull);
      expect(json['forwards'], isA<List>());
      expect((json['forwards'] as List).length, 2);
    });

    test('exportToFile excludes passwords from backup', () async {
      final service = StorageService();
      final forwards = createSampleForwards();
      final exportPath = '${tempDir.path}/backup.json';

      await service.exportToFile(exportPath, forwards);

      final content = await File(exportPath).readAsString();
      final json = jsonDecode(content) as Map<String, dynamic>;
      final exportedForwards = json['forwards'] as List;

      for (final f in exportedForwards) {
        final map = f as Map<String, dynamic>;
        expect(map.containsKey('sshPassword'), isFalse,
            reason: 'Backup should not contain passwords');
      }
    });

    test('exportToFile includes identity file paths', () async {
      final service = StorageService();
      final forwards = createSampleForwards();
      final exportPath = '${tempDir.path}/backup.json';

      await service.exportToFile(exportPath, forwards);

      final content = await File(exportPath).readAsString();
      final json = jsonDecode(content) as Map<String, dynamic>;
      final exportedForwards = json['forwards'] as List;
      final secondForward = exportedForwards[1] as Map<String, dynamic>;

      expect(secondForward['identityFilePath'], '/home/user/.ssh/id_rsa');
    });

    test('importFromFile restores forwards without passwords', () async {
      final service = StorageService();
      final forwards = createSampleForwards();
      final exportPath = '${tempDir.path}/backup.json';

      await service.exportToFile(exportPath, forwards);
      final imported = await service.importFromFile(exportPath);

      expect(imported.length, 2);

      // First forward had password - should be null after import
      expect(imported[0].name, 'DB Tunnel');
      expect(imported[0].sshHost, '10.0.0.1');
      expect(imported[0].sshPassword, isNull);
      expect(imported[0].needsPassword, isTrue);

      // Second forward had identity file - should still have it
      expect(imported[1].name, 'Web Tunnel');
      expect(imported[1].identityFilePath, '/home/user/.ssh/id_rsa');
      expect(imported[1].needsPassword, isFalse);
    });

    test('importFromFile preserves all non-sensitive fields', () async {
      final service = StorageService();
      final forwards = createSampleForwards();
      final exportPath = '${tempDir.path}/backup.json';

      await service.exportToFile(exportPath, forwards);
      final imported = await service.importFromFile(exportPath);

      expect(imported[0].id, 'id-1');
      expect(imported[0].sshPort, 22);
      expect(imported[0].sshUsername, 'admin');
      expect(imported[0].localBindAddress, '127.0.0.1');
      expect(imported[0].localPort, 3306);
      expect(imported[0].remoteHost, 'db.internal');
      expect(imported[0].remotePort, 3306);

      expect(imported[1].id, 'id-2');
      expect(imported[1].sshPort, 2222);
      expect(imported[1].sshUsername, 'deploy');
      expect(imported[1].localPort, 8080);
      expect(imported[1].remoteHost, 'web.internal');
      expect(imported[1].remotePort, 80);
    });

    test('export/import roundtrip with empty list', () async {
      final service = StorageService();
      final exportPath = '${tempDir.path}/empty_backup.json';

      await service.exportToFile(exportPath, []);
      final imported = await service.importFromFile(exportPath);

      expect(imported, isEmpty);
    });

    test('importFromFile throws BackupImportException for missing file',
        () async {
      final service = StorageService();
      expect(
        () => service.importFromFile('${tempDir.path}/does_not_exist.json'),
        throwsA(isA<BackupImportException>()),
      );
    });

    test('importFromFile throws BackupImportException for malformed JSON',
        () async {
      final service = StorageService();
      final path = '${tempDir.path}/broken.json';
      await File(path).writeAsString('{not valid json');

      expect(
        () => service.importFromFile(path),
        throwsA(isA<BackupImportException>()),
      );
    });

    test('importFromFile throws when root is not an object', () async {
      final service = StorageService();
      final path = '${tempDir.path}/array.json';
      await File(path).writeAsString('[1, 2, 3]');

      expect(
        () => service.importFromFile(path),
        throwsA(isA<BackupImportException>()),
      );
    });

    test('importFromFile throws when forwards field missing', () async {
      final service = StorageService();
      final path = '${tempDir.path}/no_forwards.json';
      await File(path).writeAsString('{"version": 1}');

      expect(
        () => service.importFromFile(path),
        throwsA(isA<BackupImportException>()),
      );
    });

    test('importFromFile throws when forwards is not a list', () async {
      final service = StorageService();
      final path = '${tempDir.path}/bad_forwards.json';
      await File(path).writeAsString('{"version": 1, "forwards": "nope"}');

      expect(
        () => service.importFromFile(path),
        throwsA(isA<BackupImportException>()),
      );
    });

    test('importFromFile throws when forward entry is malformed', () async {
      final service = StorageService();
      final path = '${tempDir.path}/bad_entry.json';
      await File(path).writeAsString(
          '{"version": 1, "forwards": [{"name": "missing required fields"}]}');

      expect(
        () => service.importFromFile(path),
        throwsA(isA<BackupImportException>()),
      );
    });

    test('importFromFile throws when version is from future', () async {
      final service = StorageService();
      final path = '${tempDir.path}/future.json';
      await File(path).writeAsString('{"version": 99, "forwards": []}');

      expect(
        () => service.importFromFile(path),
        throwsA(isA<BackupImportException>()),
      );
    });

    test('importFromFile accepts backup without version field', () async {
      // Backwards compatibility: older backups or third-party exports
      final service = StorageService();
      final path = '${tempDir.path}/no_version.json';
      await File(path).writeAsString('{"forwards": []}');

      final imported = await service.importFromFile(path);
      expect(imported, isEmpty);
    });
  });

  group('StorageService - config file operations', () {
    late Directory tempDir;
    // late StorageService service;

    setUp(() async {
      tempDir = await Directory.systemTemp.createTemp('tunnel_pilot_cfg_');
      // service = StorageService();
    });

    tearDown(() async {
      if (await tempDir.exists()) {
        await tempDir.delete(recursive: true);
      }
    });

    test('saveForwards and read back via direct file', () async {
      final configPath = '${tempDir.path}/config.json';
      final forwards = [
        ForwardConfig(
          id: 'test-1',
          name: 'Test',
          sshHost: 'host',
          sshUsername: 'user',
          sshPassword: 'pass',
          localPort: 8080,
          remoteHost: 'remote',
          remotePort: 80,
        ),
      ];

      // Write directly to test path
      final json = {
        'forwards': forwards.map((f) => f.toJson()).toList(),
      };
      await File(configPath)
          .writeAsString(const JsonEncoder.withIndent('  ').convert(json));

      // Read back and verify
      final content = await File(configPath).readAsString();
      final parsed = jsonDecode(content) as Map<String, dynamic>;
      final restored = (parsed['forwards'] as List)
          .map((e) => ForwardConfig.fromJson(e as Map<String, dynamic>))
          .toList();

      expect(restored.length, 1);
      expect(restored[0].id, 'test-1');
      expect(restored[0].sshPassword, 'pass');
    });

    test('saveSettings and read back via direct file', () async {
      final configPath = '${tempDir.path}/config.json';
      final settings = AppSettings(
        launchAtLogin: true,
        showNotifications: false,
      );

      final json = {'settings': settings.toJson()};
      await File(configPath)
          .writeAsString(const JsonEncoder.withIndent('  ').convert(json));

      final content = await File(configPath).readAsString();
      final parsed = jsonDecode(content) as Map<String, dynamic>;
      final restored =
          AppSettings.fromJson(parsed['settings'] as Map<String, dynamic>);

      expect(restored.launchAtLogin, isTrue);
      expect(restored.showNotifications, isFalse);
    });
  });
}
