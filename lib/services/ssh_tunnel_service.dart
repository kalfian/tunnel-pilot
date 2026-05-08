import 'dart:async';
import 'dart:io';

import 'package:dartssh2/dartssh2.dart';

import '../models/forward_config.dart';
import '../models/forward_status.dart';
import '../models/tunnel_stats.dart';
import 'counting_socket_wrapper.dart';

class TunnelConnection {
  final SSHClient client;
  final ServerSocket serverSocket;
  final List<Socket> activeSockets = [];
  final List<CountingSocketWrapper> _counters = [];
  final void Function(ForwardStatus status, String? error) onStatus;
  final int keepAliveMaxFailures;
  StreamSubscription<Socket>? serverSubscription;

  DateTime? connectedSince;
  Duration? lastPingLatency;

  int consecutiveForwardFailures = 0;
  int consecutivePingFailures = 0;

  static const maxForwardFailures = 3;

  TunnelConnection({
    required this.client,
    required this.serverSocket,
    required this.onStatus,
    required this.keepAliveMaxFailures,
  });

  TunnelStats getStats() {
    int totalUp = 0, totalDown = 0;
    for (final c in _counters) {
      totalUp += c.bytesUp;
      totalDown += c.bytesDown;
    }
    return TunnelStats(
      activeConnections: activeSockets.length,
      totalBytesUp: totalUp,
      totalBytesDown: totalDown,
      lastPingLatency: lastPingLatency,
      connectedSince: connectedSince,
    );
  }
}

typedef StatusCallback = void Function(
    String id, ForwardStatus status, String? errorMessage);

class SshTunnelService {
  final Map<String, TunnelConnection> _connections = {};
  final Map<String, int> _generation = {};

  void Function(String id, TunnelStats stats)? onStatsUpdate;

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
    if (_healthCheckRunning) return;
    _healthCheckRunning = true;

    try {
      final entries = _connections.entries.toList();
      if (entries.isEmpty) return;

      await Future.wait(entries.map((entry) async {
        final id = entry.key;
        final conn = entry.value;

        if (!_connections.containsKey(id) || _connections[id] != conn) return;

        if (conn.client.isClosed) {
          _cleanupTunnel(id, conn);
          conn.onStatus(ForwardStatus.error, 'SSH connection lost');
          return;
        }

        try {
          final sw = Stopwatch()..start();
          await conn.client.ping().timeout(_pingTimeout);
          sw.stop();
          conn.lastPingLatency = sw.elapsed;
          conn.consecutivePingFailures = 0;
        } catch (_) {
          final current = _connections[id];
          if (current == null || current.client != conn.client) return;
          current.consecutivePingFailures++;
          if (current.consecutivePingFailures >= current.keepAliveMaxFailures) {
            _cleanupTunnel(id, current);
            current.onStatus(ForwardStatus.error, 'SSH connection lost');
          }
        }
      }));

      for (final entry in _connections.entries) {
        onStatsUpdate?.call(entry.key, entry.value.getStats());
      }
    } finally {
      _healthCheckRunning = false;
      _stopHealthMonitorIfIdle();
    }
  }

  Future<void> _cleanupTunnel(String id, TunnelConnection conn) async {
    _connections.remove(id);
    await conn.serverSubscription?.cancel();
    conn.serverSubscription = null;
    for (final s in conn.activeSockets) {
      s.destroy();
    }
    conn.activeSockets.clear();
    for (final c in conn._counters) {
      c.dispose();
    }
    conn._counters.clear();
    try {
      await conn.serverSocket.close();
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
      // 1. Robust binding with retry to handle OS race conditions (e.g. TIME_WAIT)
      // We bind FIRST to reserve the port before wasting time on SSH handshake
      ServerSocket? serverSocket;
      Object? bindError;
      for (var i = 0; i < 5; i++) {
        try {
          serverSocket = await ServerSocket.bind(
            config.localBindAddress,
            config.localPort,
            shared: true,
          );
          break;
        } catch (e) {
          bindError = e;
          final errorStr = e.toString().toLowerCase();
          final isAddrInUse = errorStr.contains('address already in use') || 
                             errorStr.contains('eaddrinuse') ||
                             errorStr.contains('shared flag') ||
                             (e is SocketException && (e.osError?.errorCode == 48 || e.osError?.errorCode == 98));

          if (isAddrInUse && i < 4) {
            await Future.delayed(const Duration(milliseconds: 500));
            continue;
          }
          rethrow;
        }
      }

      if (serverSocket == null) {
        throw bindError ?? Exception('Failed to bind to local port');
      }

      // 2. Start SSH connection
      final socket = await SSHSocket.connect(config.sshHost, config.sshPort)
          .timeout(const Duration(seconds: 15));

      SSHClient client;

      final keepAlive = config.keepAliveIntervalSec > 0
          ? Duration(seconds: config.keepAliveIntervalSec)
          : const Duration(seconds: 10); // Default to 10s for faster VPN detection

      if (config.identityFilePath != null &&
          config.identityFilePath!.isNotEmpty) {
        final keyFile = File(config.identityFilePath!);
        final keyContent = await keyFile.readAsString();
        client = SSHClient(
          socket,
          username: config.sshUsername,
          identities: SSHKeyPair.fromPem(keyContent),
          keepAliveInterval: keepAlive,
        );
      } else {
        client = SSHClient(
          socket,
          username: config.sshUsername,
          onPasswordRequest: () => config.sshPassword ?? '',
          keepAliveInterval: keepAlive,
        );
      }

      final tunnel = TunnelConnection(
        client: client,
        serverSocket: serverSocket,
        onStatus: safeCallback,
        keepAliveMaxFailures:
            config.keepAliveMaxCount > 0 ? config.keepAliveMaxCount : 3, // Faster detection
      );
      _connections[config.id] = tunnel;

      void onConnectionLost(_) async {
        final conn = _connections[config.id];
        if (conn == null || conn.client != client) return;
        await _cleanupTunnel(config.id, conn);
        safeCallback(ForwardStatus.error, 'SSH connection lost');
        _stopHealthMonitorIfIdle();
      }

      client.done.then(onConnectionLost).catchError(onConnectionLost);

      tunnel.serverSubscription = serverSocket.listen(
        (localSocket) async {
          final channelFuture = client.forwardLocal(
            config.remoteHost,
            config.remotePort,
          );
          try {
            final channel = await channelFuture
                .timeout(const Duration(seconds: 10));

            tunnel.consecutiveForwardFailures = 0;
            tunnel.activeSockets.add(localSocket);

            final counter = CountingSocketWrapper();
            tunnel._counters.add(counter);

            counter.pipeChannelToLocal(
                channel.stream.cast<List<int>>(), localSocket);
            counter.pipeLocalToChannel(localSocket, channel.sink);

            localSocket.done.then((_) {
              tunnel.activeSockets.remove(localSocket);
              tunnel._counters.remove(counter);
              counter.dispose();
            }).catchError((_) {
              tunnel.activeSockets.remove(localSocket);
              tunnel._counters.remove(counter);
              counter.dispose();
            });
          } catch (e) {
            if (e is TimeoutException) {
              channelFuture.then((ch) {
                try {
                  ch.sink.close();
                } catch (_) {}
              }).catchError((_) {});
            }
            localSocket.destroy();

            final conn = _connections[config.id];
            if (conn != null && conn.client == client) {
              conn.consecutiveForwardFailures++;
              if (conn.consecutiveForwardFailures >=
                  TunnelConnection.maxForwardFailures) {
                await _cleanupTunnel(config.id, conn);
                safeCallback(ForwardStatus.error,
                    'Port forwarding failed ($e)');
                _stopHealthMonitorIfIdle();
              }
            }
          }
        },
        onError: (error) async {
          final conn = _connections[config.id];
          if (conn != null && conn.client == client) {
             await _cleanupTunnel(config.id, conn);
          }
          safeCallback(ForwardStatus.error, error.toString());
        },
        onDone: () async {
          final conn = _connections[config.id];
          if (conn != null && conn.client == client) {
            await _cleanupTunnel(config.id, conn);
            safeCallback(ForwardStatus.disconnected, null);
            _stopHealthMonitorIfIdle();
          }
        },
      );

      tunnel.connectedSince = DateTime.now();
      safeCallback(ForwardStatus.connected, null);

      _startHealthMonitor();
    } catch (e) {
      final conn = _connections[config.id];
      if (conn != null) {
        await _cleanupTunnel(config.id, conn);
      }
      _connections.remove(config.id);
      safeCallback(ForwardStatus.error, e.toString());
    }
  }

  Future<void> disconnect(String id) async {
    _generation[id] = (_generation[id] ?? 0) + 1;

    final tunnel = _connections[id];
    if (tunnel == null) {
      _generation.remove(id);
      return;
    }

    await _cleanupTunnel(id, tunnel);
    _stopHealthMonitorIfIdle();
  }

  Future<void> disconnectAll() async {
    final ids = _connections.keys.toList();
    await Future.wait(ids.map((id) => disconnect(id)));
  }

  bool isConnected(String id) => _connections.containsKey(id);

  TunnelStats? getStats(String id) => _connections[id]?.getStats();

  void dispose() {
    _healthCheckTimer?.cancel();
    _healthCheckTimer = null;
    final ids = _connections.keys.toList();
    for (final id in ids) {
      final conn = _connections.remove(id);
      if (conn != null) {
        conn.serverSubscription?.cancel();
        conn.serverSubscription = null;
        for (final s in conn.activeSockets) {
          s.destroy();
        }
        for (final c in conn._counters) {
          c.dispose();
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
