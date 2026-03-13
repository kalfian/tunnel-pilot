import 'dart:async';
import 'dart:io';

import 'package:dartssh2/dartssh2.dart';

import '../models/forward_config.dart';
import '../models/forward_status.dart';

class TunnelConnection {
  final SSHClient client;
  final ServerSocket serverSocket;
  final List<Socket> activeSockets = [];
  final void Function(ForwardStatus status, String? error) onStatus;

  TunnelConnection({
    required this.client,
    required this.serverSocket,
    required this.onStatus,
  });
}

typedef StatusCallback = void Function(
    String id, ForwardStatus status, String? errorMessage);

class SshTunnelService {
  final Map<String, TunnelConnection> _connections = {};
  final Map<String, int> _generation = {};

  // Single global health monitor — checks ALL tunnels every 3 seconds.
  // One Timer regardless of tunnel count. Only runs while tunnels are active.
  Timer? _healthCheckTimer;
  bool _healthCheckRunning = false;
  static const _healthInterval = Duration(seconds: 3);
  static const _pingTimeout = Duration(seconds: 3);

  void _startHealthMonitor() {
    _healthCheckTimer ??= Timer.periodic(_healthInterval, (_) => _checkAll());
  }

  void _stopHealthMonitorIfIdle() {
    if (_connections.isEmpty) {
      _healthCheckTimer?.cancel();
      _healthCheckTimer = null;
    }
  }

  Future<void> _checkAll() async {
    if (_healthCheckRunning) return; // skip if previous check still in flight
    _healthCheckRunning = true;

    try {
      final entries = _connections.entries.toList();
      if (entries.isEmpty) return;

      await Future.wait(entries.map((entry) async {
        final id = entry.key;
        final conn = entry.value;

        // Already cleaned up by another path
        if (!_connections.containsKey(id) || _connections[id] != conn) return;

        if (conn.client.isClosed) {
          _cleanupTunnel(id, conn);
          conn.onStatus(ForwardStatus.error, 'SSH connection lost');
          return;
        }

        try {
          await conn.client.ping().timeout(_pingTimeout);
        } catch (_) {
          // Verify tunnel still belongs to this connection
          final current = _connections[id];
          if (current != null && current.client == conn.client) {
            _cleanupTunnel(id, current);
            current.onStatus(ForwardStatus.error, 'SSH connection lost');
          }
        }
      }));
    } finally {
      _healthCheckRunning = false;
      _stopHealthMonitorIfIdle();
    }
  }

  void _cleanupTunnel(String id, TunnelConnection conn) {
    _connections.remove(id);
    for (final s in conn.activeSockets) {
      s.destroy();
    }
    conn.activeSockets.clear();
    try {
      conn.serverSocket.close();
    } catch (_) {}
    try {
      conn.client.close();
    } catch (_) {}
  }

  Future<void> connect(
    ForwardConfig config, {
    required StatusCallback onStatusChanged,
  }) async {
    final gen = (_generation[config.id] ?? 0) + 1;
    _generation[config.id] = gen;

    if (_connections.containsKey(config.id)) {
      await disconnect(config.id);
    }

    void safeCallback(ForwardStatus status, String? error) {
      if ((_generation[config.id] ?? 0) != gen) return;
      onStatusChanged(config.id, status, error);
    }

    safeCallback(ForwardStatus.connecting, null);

    try {
      final socket = await SSHSocket.connect(config.sshHost, config.sshPort)
          .timeout(const Duration(seconds: 15));

      SSHClient client;

      if (config.identityFilePath != null &&
          config.identityFilePath!.isNotEmpty) {
        final keyFile = File(config.identityFilePath!);
        final keyContent = await keyFile.readAsString();
        client = SSHClient(
          socket,
          username: config.sshUsername,
          identities: SSHKeyPair.fromPem(keyContent),
          keepAliveInterval: null,
        );
      } else {
        client = SSHClient(
          socket,
          username: config.sshUsername,
          onPasswordRequest: () => config.sshPassword ?? '',
          keepAliveInterval: null,
        );
      }

      final serverSocket = await ServerSocket.bind(
        config.localBindAddress,
        config.localPort,
        shared: true,
      );

      final tunnel = TunnelConnection(
        client: client,
        serverSocket: serverSocket,
        onStatus: safeCallback,
      );
      _connections[config.id] = tunnel;

      // Backup: listen for SSH transport close (fires when TCP detects closure)
      void onConnectionLost(_) {
        final conn = _connections[config.id];
        if (conn == null || conn.client != client) return;
        _cleanupTunnel(config.id, conn);
        safeCallback(ForwardStatus.error, 'SSH connection lost');
        _stopHealthMonitorIfIdle();
      }

      client.done.then(onConnectionLost).catchError(onConnectionLost);

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
          safeCallback(ForwardStatus.error, error.toString());
        },
        onDone: () {
          if (_connections.containsKey(config.id)) {
            final t = _connections.remove(config.id);
            safeCallback(ForwardStatus.disconnected, null);
            _stopHealthMonitorIfIdle();
          }
        },
      );

      safeCallback(ForwardStatus.connected, null);

      // Start the global health monitor (single timer for all tunnels)
      _startHealthMonitor();
    } catch (e) {
      _connections.remove(config.id);
      safeCallback(ForwardStatus.error, e.toString());
    }
  }

  Future<void> disconnect(String id) async {
    _generation[id] = (_generation[id] ?? 0) + 1;

    final tunnel = _connections.remove(id);
    if (tunnel == null) {
      _generation.remove(id);
      return;
    }

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

    _stopHealthMonitorIfIdle();
  }

  Future<void> disconnectAll() async {
    final ids = _connections.keys.toList();
    for (final id in ids) {
      await disconnect(id);
    }
  }

  bool isConnected(String id) => _connections.containsKey(id);

  void dispose() {
    _healthCheckTimer?.cancel();
    _healthCheckTimer = null;
    final ids = _connections.keys.toList();
    for (final id in ids) {
      final conn = _connections.remove(id);
      if (conn != null) {
        for (final s in conn.activeSockets) {
          s.destroy();
        }
        try {
          conn.serverSocket.close();
        } catch (_) {}
        try {
          conn.client.close();
        } catch (_) {}
      }
    }
    _generation.clear();
  }

  Future<bool> isAlive(String id) async {
    final tunnel = _connections[id];
    if (tunnel == null) return false;
    if (tunnel.client.isClosed) return false;

    try {
      await tunnel.client.ping().timeout(_pingTimeout);
      return true;
    } catch (_) {
      return false;
    }
  }
}
