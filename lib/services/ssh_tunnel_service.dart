import 'dart:async';
import 'dart:io';

import 'package:dartssh2/dartssh2.dart';

import '../models/forward_config.dart';
import '../models/forward_status.dart';

class TunnelConnection {
  final SSHClient client;
  final ServerSocket serverSocket;
  final List<Socket> activeSockets = [];
  Timer? keepAliveTimer;
  int missedAlives = 0;

  TunnelConnection({required this.client, required this.serverSocket});
}

typedef StatusCallback = void Function(
    String id, ForwardStatus status, String? errorMessage);

class SshTunnelService {
  final Map<String, TunnelConnection> _connections = {};

  Future<void> connect(
    ForwardConfig config, {
    required StatusCallback onStatusChanged,
  }) async {
    if (_connections.containsKey(config.id)) {
      await disconnect(config.id);
    }

    onStatusChanged(config.id, ForwardStatus.connecting, null);

    try {
      final socket = await SSHSocket.connect(config.sshHost, config.sshPort);

      SSHClient client;

      if (config.identityFilePath != null &&
          config.identityFilePath!.isNotEmpty) {
        final keyFile = File(config.identityFilePath!);
        final keyContent = await keyFile.readAsString();
        client = SSHClient(
          socket,
          username: config.sshUsername,
          identities: SSHKeyPair.fromPem(keyContent),
        );
      } else {
        client = SSHClient(
          socket,
          username: config.sshUsername,
          onPasswordRequest: () => config.sshPassword ?? '',
        );
      }

      final serverSocket = await ServerSocket.bind(
        config.localBindAddress,
        config.localPort,
      );

      final tunnel = TunnelConnection(
        client: client,
        serverSocket: serverSocket,
      );
      _connections[config.id] = tunnel;

      // Start keep-alive timer
      if (config.keepAliveIntervalSec > 0) {
        tunnel.missedAlives = 0;
        tunnel.keepAliveTimer = Timer.periodic(
          Duration(seconds: config.keepAliveIntervalSec),
          (_) async {
            try {
              await client.execute('');
              tunnel.missedAlives = 0;
            } catch (_) {
              tunnel.missedAlives++;
              if (tunnel.missedAlives >= config.keepAliveMaxCount) {
                tunnel.keepAliveTimer?.cancel();
                _connections.remove(config.id);
                for (final s in tunnel.activeSockets) {
                  s.destroy();
                }
                tunnel.activeSockets.clear();
                try {
                  await tunnel.serverSocket.close();
                } catch (_) {}
                try {
                  client.close();
                } catch (_) {}
                onStatusChanged(
                  config.id,
                  ForwardStatus.error,
                  'Connection lost: ${config.keepAliveMaxCount} unanswered keep-alive messages',
                );
              }
            }
          },
        );
      }

      serverSocket.listen(
        (localSocket) async {
          try {
            final channel = await client.forwardLocal(
              config.remoteHost,
              config.remotePort,
            );

            tunnel.activeSockets.add(localSocket);

            channel.stream.cast<List<int>>().listen(
              localSocket.add,
              onError: (_) => localSocket.destroy(),
              onDone: () => localSocket.close(),
            );
            localSocket.listen(
              channel.sink.add,
              onError: (_) => channel.sink.close(),
              onDone: () => channel.sink.close(),
            );

            localSocket.done.then((_) {
              tunnel.activeSockets.remove(localSocket);
            }).catchError((_) {
              tunnel.activeSockets.remove(localSocket);
            });
          } catch (e) {
            localSocket.destroy();
          }
        },
        onError: (error) {
          onStatusChanged(config.id, ForwardStatus.error, error.toString());
        },
        onDone: () {
          if (_connections.containsKey(config.id)) {
            final t = _connections.remove(config.id);
            t?.keepAliveTimer?.cancel();
            onStatusChanged(config.id, ForwardStatus.disconnected, null);
          }
        },
      );

      onStatusChanged(config.id, ForwardStatus.connected, null);
    } catch (e) {
      _connections.remove(config.id);
      onStatusChanged(config.id, ForwardStatus.error, e.toString());
    }
  }

  Future<void> disconnect(String id) async {
    final tunnel = _connections.remove(id);
    if (tunnel == null) return;

    tunnel.keepAliveTimer?.cancel();

    for (final socket in tunnel.activeSockets) {
      socket.destroy();
    }
    tunnel.activeSockets.clear();

    try {
      await tunnel.serverSocket.close();
    } catch (_) {}

    try {
      tunnel.client.close();
    } catch (_) {}
  }

  Future<void> disconnectAll() async {
    final ids = _connections.keys.toList();
    for (final id in ids) {
      await disconnect(id);
    }
  }

  bool isConnected(String id) => _connections.containsKey(id);
}
