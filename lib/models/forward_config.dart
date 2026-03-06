import 'package:uuid/uuid.dart';

class ForwardConfig {
  final String id;
  String name;
  String sshHost;
  int sshPort;
  String sshUsername;
  String? sshPassword;
  String? identityFilePath;
  String localBindAddress;
  int localPort;
  String remoteHost;
  int remotePort;

  ForwardConfig({
    String? id,
    required this.name,
    required this.sshHost,
    this.sshPort = 22,
    required this.sshUsername,
    this.sshPassword,
    this.identityFilePath,
    this.localBindAddress = '127.0.0.1',
    required this.localPort,
    required this.remoteHost,
    required this.remotePort,
  }) : id = id ?? const Uuid().v4();

  Map<String, dynamic> toJson() => {
        'id': id,
        'name': name,
        'sshHost': sshHost,
        'sshPort': sshPort,
        'sshUsername': sshUsername,
        'sshPassword': sshPassword,
        'identityFilePath': identityFilePath,
        'localBindAddress': localBindAddress,
        'localPort': localPort,
        'remoteHost': remoteHost,
        'remotePort': remotePort,
      };

  Map<String, dynamic> toJsonForBackup() => {
        'id': id,
        'name': name,
        'sshHost': sshHost,
        'sshPort': sshPort,
        'sshUsername': sshUsername,
        'identityFilePath': identityFilePath,
        'localBindAddress': localBindAddress,
        'localPort': localPort,
        'remoteHost': remoteHost,
        'remotePort': remotePort,
      };

  factory ForwardConfig.fromJson(Map<String, dynamic> json) => ForwardConfig(
        id: json['id'] as String?,
        name: json['name'] as String,
        sshHost: json['sshHost'] as String,
        sshPort: json['sshPort'] as int? ?? 22,
        sshUsername: json['sshUsername'] as String,
        sshPassword: json['sshPassword'] as String?,
        identityFilePath: json['identityFilePath'] as String?,
        localBindAddress: json['localBindAddress'] as String? ?? '127.0.0.1',
        localPort: json['localPort'] as int,
        remoteHost: json['remoteHost'] as String,
        remotePort: json['remotePort'] as int,
      );

  ForwardConfig copyWith({
    String? id,
    String? name,
    String? sshHost,
    int? sshPort,
    String? sshUsername,
    String? sshPassword,
    String? identityFilePath,
    String? localBindAddress,
    int? localPort,
    String? remoteHost,
    int? remotePort,
  }) =>
      ForwardConfig(
        id: id ?? this.id,
        name: name ?? this.name,
        sshHost: sshHost ?? this.sshHost,
        sshPort: sshPort ?? this.sshPort,
        sshUsername: sshUsername ?? this.sshUsername,
        sshPassword: sshPassword ?? this.sshPassword,
        identityFilePath: identityFilePath ?? this.identityFilePath,
        localBindAddress: localBindAddress ?? this.localBindAddress,
        localPort: localPort ?? this.localPort,
        remoteHost: remoteHost ?? this.remoteHost,
        remotePort: remotePort ?? this.remotePort,
      );

  bool get needsPassword =>
      sshPassword == null &&
      (identityFilePath == null || identityFilePath!.isEmpty);
}
