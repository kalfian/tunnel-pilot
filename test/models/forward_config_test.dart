import 'package:flutter_test/flutter_test.dart';
import 'package:tunnel_pilot/models/forward_config.dart';

void main() {
  group('ForwardConfig', () {
    ForwardConfig createSample({
      String? id,
      String? sshPassword,
      String? identityFilePath,
    }) {
      return ForwardConfig(
        id: id ?? 'test-id-123',
        name: 'My Server',
        sshHost: '192.168.1.100',
        sshPort: 22,
        sshUsername: 'admin',
        sshPassword: sshPassword,
        identityFilePath: identityFilePath,
        localBindAddress: '127.0.0.1',
        localPort: 3306,
        remoteHost: 'db.internal',
        remotePort: 3306,
      );
    }

    test('creates with all fields', () {
      final config = createSample(sshPassword: 'secret123');

      expect(config.id, 'test-id-123');
      expect(config.name, 'My Server');
      expect(config.sshHost, '192.168.1.100');
      expect(config.sshPort, 22);
      expect(config.sshUsername, 'admin');
      expect(config.sshPassword, 'secret123');
      expect(config.identityFilePath, isNull);
      expect(config.localBindAddress, '127.0.0.1');
      expect(config.localPort, 3306);
      expect(config.remoteHost, 'db.internal');
      expect(config.remotePort, 3306);
    });

    test('generates UUID if id not provided', () {
      final config = ForwardConfig(
        name: 'Test',
        sshHost: 'host',
        sshUsername: 'user',
        localPort: 8080,
        remoteHost: 'remote',
        remotePort: 80,
      );
      expect(config.id, isNotEmpty);
      expect(config.id.length, greaterThan(10));
    });

    test('uses default sshPort 22', () {
      final config = ForwardConfig(
        name: 'Test',
        sshHost: 'host',
        sshUsername: 'user',
        localPort: 8080,
        remoteHost: 'remote',
        remotePort: 80,
      );
      expect(config.sshPort, 22);
    });

    test('uses default localBindAddress 127.0.0.1', () {
      final config = ForwardConfig(
        name: 'Test',
        sshHost: 'host',
        sshUsername: 'user',
        localPort: 8080,
        remoteHost: 'remote',
        remotePort: 80,
      );
      expect(config.localBindAddress, '127.0.0.1');
    });

    group('toJson', () {
      test('serializes all fields including password', () {
        final config = createSample(sshPassword: 'secret123');
        final json = config.toJson();

        expect(json['id'], 'test-id-123');
        expect(json['name'], 'My Server');
        expect(json['sshHost'], '192.168.1.100');
        expect(json['sshPort'], 22);
        expect(json['sshUsername'], 'admin');
        expect(json['sshPassword'], 'secret123');
        expect(json['identityFilePath'], isNull);
        expect(json['localBindAddress'], '127.0.0.1');
        expect(json['localPort'], 3306);
        expect(json['remoteHost'], 'db.internal');
        expect(json['remotePort'], 3306);
      });

      test('serializes null password as null', () {
        final config = createSample();
        final json = config.toJson();
        expect(json['sshPassword'], isNull);
      });

      test('serializes identity file path', () {
        final config =
            createSample(identityFilePath: '/home/user/.ssh/id_rsa');
        final json = config.toJson();
        expect(json['identityFilePath'], '/home/user/.ssh/id_rsa');
      });
    });

    group('toJsonForBackup', () {
      test('excludes password', () {
        final config = createSample(sshPassword: 'secret123');
        final json = config.toJsonForBackup();

        expect(json.containsKey('sshPassword'), isFalse);
        expect(json['id'], 'test-id-123');
        expect(json['name'], 'My Server');
        expect(json['sshHost'], '192.168.1.100');
      });

      test('includes identity file path', () {
        final config =
            createSample(identityFilePath: '/home/user/.ssh/id_rsa');
        final json = config.toJsonForBackup();

        expect(json['identityFilePath'], '/home/user/.ssh/id_rsa');
        expect(json.containsKey('sshPassword'), isFalse);
      });

      test('includes all non-sensitive fields', () {
        final config = createSample(sshPassword: 'secret');
        final json = config.toJsonForBackup();

        expect(json['id'], isNotNull);
        expect(json['name'], isNotNull);
        expect(json['sshHost'], isNotNull);
        expect(json['sshPort'], isNotNull);
        expect(json['sshUsername'], isNotNull);
        expect(json['localBindAddress'], isNotNull);
        expect(json['localPort'], isNotNull);
        expect(json['remoteHost'], isNotNull);
        expect(json['remotePort'], isNotNull);
      });
    });

    group('fromJson', () {
      test('deserializes all fields', () {
        final json = {
          'id': 'abc-123',
          'name': 'Production DB',
          'sshHost': '10.0.0.1',
          'sshPort': 2222,
          'sshUsername': 'deploy',
          'sshPassword': 'p@ss',
          'identityFilePath': null,
          'localBindAddress': '0.0.0.0',
          'localPort': 5432,
          'remoteHost': 'pg.internal',
          'remotePort': 5432,
        };

        final config = ForwardConfig.fromJson(json);

        expect(config.id, 'abc-123');
        expect(config.name, 'Production DB');
        expect(config.sshHost, '10.0.0.1');
        expect(config.sshPort, 2222);
        expect(config.sshUsername, 'deploy');
        expect(config.sshPassword, 'p@ss');
        expect(config.identityFilePath, isNull);
        expect(config.localBindAddress, '0.0.0.0');
        expect(config.localPort, 5432);
        expect(config.remoteHost, 'pg.internal');
        expect(config.remotePort, 5432);
      });

      test('uses defaults for missing optional fields', () {
        final json = {
          'name': 'Test',
          'sshHost': 'host',
          'sshUsername': 'user',
          'localPort': 8080,
          'remoteHost': 'remote',
          'remotePort': 80,
        };

        final config = ForwardConfig.fromJson(json);

        expect(config.sshPort, 22);
        expect(config.localBindAddress, '127.0.0.1');
        expect(config.sshPassword, isNull);
        expect(config.identityFilePath, isNull);
      });

      test('handles backup JSON (no password field)', () {
        final backupJson = {
          'id': 'backup-id',
          'name': 'Backup Server',
          'sshHost': 'backup.host',
          'sshPort': 22,
          'sshUsername': 'backupuser',
          'identityFilePath': '/path/to/key',
          'localBindAddress': '127.0.0.1',
          'localPort': 9090,
          'remoteHost': 'internal.host',
          'remotePort': 9090,
        };

        final config = ForwardConfig.fromJson(backupJson);

        expect(config.sshPassword, isNull);
        expect(config.identityFilePath, '/path/to/key');
        expect(config.name, 'Backup Server');
      });
    });

    group('roundtrip', () {
      test('toJson -> fromJson preserves all data', () {
        final original = createSample(sshPassword: 'secret');
        final json = original.toJson();
        final restored = ForwardConfig.fromJson(json);

        expect(restored.id, original.id);
        expect(restored.name, original.name);
        expect(restored.sshHost, original.sshHost);
        expect(restored.sshPort, original.sshPort);
        expect(restored.sshUsername, original.sshUsername);
        expect(restored.sshPassword, original.sshPassword);
        expect(restored.localBindAddress, original.localBindAddress);
        expect(restored.localPort, original.localPort);
        expect(restored.remoteHost, original.remoteHost);
        expect(restored.remotePort, original.remotePort);
      });

      test('backup roundtrip loses password', () {
        final original = createSample(sshPassword: 'secret');
        final backupJson = original.toJsonForBackup();
        final restored = ForwardConfig.fromJson(backupJson);

        expect(restored.sshPassword, isNull);
        expect(restored.name, original.name);
      });
    });

    group('copyWith', () {
      test('creates copy with new id and name', () {
        final original = createSample(sshPassword: 'secret');
        final copy = original.copyWith(id: 'new-id', name: 'Copy of Server');

        expect(copy.id, 'new-id');
        expect(copy.name, 'Copy of Server');
        expect(copy.sshHost, original.sshHost);
        expect(copy.sshPort, original.sshPort);
        expect(copy.sshUsername, original.sshUsername);
        expect(copy.sshPassword, original.sshPassword);
        expect(copy.localPort, original.localPort);
        expect(copy.remoteHost, original.remoteHost);
        expect(copy.remotePort, original.remotePort);
      });

      test('preserves all fields when no arguments given', () {
        final original = createSample(sshPassword: 'pw');
        final copy = original.copyWith();

        expect(copy.id, original.id);
        expect(copy.name, original.name);
        expect(copy.sshPassword, original.sshPassword);
      });

      test('overrides specific fields', () {
        final original = createSample();
        final copy = original.copyWith(
          sshHost: 'new-host',
          localPort: 9999,
        );

        expect(copy.sshHost, 'new-host');
        expect(copy.localPort, 9999);
        expect(copy.name, original.name);
      });
    });

    group('toSshCommand', () {
      test('generates command with default bind and no identity', () {
        final config = createSample();
        expect(
          config.toSshCommand(),
          'ssh -N -L 3306:db.internal:3306 -p 22 admin@192.168.1.100',
        );
      });

      test('includes bind address when not 127.0.0.1', () {
        final config = ForwardConfig(
          id: 'x',
          name: 'X',
          sshHost: 'host',
          sshUsername: 'user',
          localBindAddress: '0.0.0.0',
          localPort: 8080,
          remoteHost: 'remote',
          remotePort: 80,
        );
        expect(
          config.toSshCommand(),
          'ssh -N -L 0.0.0.0:8080:remote:80 -p 22 user@host',
        );
      });

      test('includes identity file flag when set', () {
        final config = createSample(identityFilePath: '/home/u/.ssh/id_rsa');
        expect(
          config.toSshCommand(),
          'ssh -N -L 3306:db.internal:3306 -p 22 -i /home/u/.ssh/id_rsa admin@192.168.1.100',
        );
      });

      test('quotes identity path containing spaces', () {
        final config =
            createSample(identityFilePath: '/Users/my user/.ssh/id_rsa');
        expect(
          config.toSshCommand(),
          'ssh -N -L 3306:db.internal:3306 -p 22 -i "/Users/my user/.ssh/id_rsa" admin@192.168.1.100',
        );
      });

      test('omits identity flag when path is empty', () {
        final config = createSample(identityFilePath: '');
        expect(
          config.toSshCommand(),
          'ssh -N -L 3306:db.internal:3306 -p 22 admin@192.168.1.100',
        );
      });

      test('uses custom sshPort when non-default', () {
        final config = ForwardConfig(
          id: 'x',
          name: 'X',
          sshHost: 'host',
          sshPort: 2222,
          sshUsername: 'user',
          localPort: 8080,
          remoteHost: 'remote',
          remotePort: 80,
        );
        expect(
          config.toSshCommand(),
          'ssh -N -L 8080:remote:80 -p 2222 user@host',
        );
      });
    });

    group('needsPassword', () {
      test('returns true when no password and no identity file', () {
        final config = createSample();
        expect(config.needsPassword, isTrue);
      });

      test('returns false when password is set', () {
        final config = createSample(sshPassword: 'mypass');
        expect(config.needsPassword, isFalse);
      });

      test('returns false when identity file is set', () {
        final config =
            createSample(identityFilePath: '/home/user/.ssh/id_rsa');
        expect(config.needsPassword, isFalse);
      });

      test('returns false when both password and identity file are set', () {
        final config = createSample(
          sshPassword: 'pass',
          identityFilePath: '/path/to/key',
        );
        expect(config.needsPassword, isFalse);
      });

      test('returns true when identity file path is empty string', () {
        final config = createSample(identityFilePath: '');
        expect(config.needsPassword, isTrue);
      });
    });
  });
}
