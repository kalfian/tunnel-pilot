import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:uuid/uuid.dart';

import '../models/forward_config.dart';
import '../models/forward_status.dart';
import '../models/tunnel_stats.dart';
import '../services/log_service.dart';
import '../services/notification_service.dart';
import '../services/ssh_tunnel_service.dart';
import '../services/storage_service.dart';

class ForwardProvider extends ChangeNotifier {
  final StorageService _storage;
  final SshTunnelService _tunnel;
  final NotificationService _notification;
  final LogService _logService;
  bool _notificationsEnabled;
  bool _autoReconnect;
  int _autoReconnectDelaySec;
  int _autoReconnectMaxRetries;

  List<ForwardConfig> _forwards = [];
  final Map<String, ForwardStatus> _statuses = {};
  final Map<String, String> _errorMessages = {};
  final Map<String, int> _reconnectAttempts = {};
  final Map<String, Timer> _reconnectTimers = {};
  final Set<String> _userDisconnected = {};
  final Map<String, TunnelStats> _stats = {};

  ForwardProvider({
    required StorageService storage,
    required SshTunnelService tunnel,
    required NotificationService notification,
    required LogService logService,
    bool notificationsEnabled = true,
    bool autoReconnect = true,
    int autoReconnectDelaySec = 5,
    int autoReconnectMaxRetries = 3,
  })  : _storage = storage,
        _tunnel = tunnel,
        _notification = notification,
        _logService = logService,
        _notificationsEnabled = notificationsEnabled,
        _autoReconnect = autoReconnect,
        _autoReconnectDelaySec = autoReconnectDelaySec,
        _autoReconnectMaxRetries = autoReconnectMaxRetries {
    _tunnel.onStatsUpdate = _onStatsUpdate;
  }

  void _onStatsUpdate(String id, TunnelStats stats) {
    final prev = _stats[id];
    if (prev == stats) return;
    _stats[id] = stats;
    notifyListeners();
  }

  List<ForwardConfig> get forwards => List.unmodifiable(_forwards);

  ForwardStatus getStatus(String id) =>
      _statuses[id] ?? ForwardStatus.disconnected;

  String? getErrorMessage(String id) => _errorMessages[id];

  TunnelStats? getStats(String id) => _stats[id];

  set notificationsEnabled(bool value) => _notificationsEnabled = value;

  set autoReconnect(bool value) => _autoReconnect = value;

  set autoReconnectDelaySec(int value) => _autoReconnectDelaySec = value;

  set autoReconnectMaxRetries(int value) => _autoReconnectMaxRetries = value;

  Future<void> loadForwards(List<ForwardConfig> forwards) async {
    _forwards = forwards;
    notifyListeners();
  }

  Future<void> addForward(ForwardConfig config) async {
    _forwards.add(config);
    await _storage.saveForwards(_forwards);
    notifyListeners();
  }

  Future<void> updateForward(ForwardConfig config) async {
    final index = _forwards.indexWhere((f) => f.id == config.id);
    if (index == -1) return;

    final wasConnected = getStatus(config.id) == ForwardStatus.connected;
    if (wasConnected) {
      _statuses[config.id] = ForwardStatus.disconnecting;
      notifyListeners();
      await _tunnel.disconnect(config.id);
      _statuses[config.id] = ForwardStatus.disconnected;
      _stats.remove(config.id);
      _logService.info(config.name, 'Disconnected (config updated)');
    }

    _forwards[index] = config;
    await _storage.saveForwards(_forwards);
    notifyListeners();
  }

  Future<void> removeForward(String id) async {
    final config = _forwards.firstWhere((f) => f.id == id);
    _cancelReconnect(id);
    if (getStatus(id) == ForwardStatus.connected) {
      _statuses[id] = ForwardStatus.disconnecting;
      notifyListeners();
    }
    await _tunnel.disconnect(id);
    _forwards.removeWhere((f) => f.id == id);
    _statuses.remove(id);
    _errorMessages.remove(id);
    _userDisconnected.remove(id);
    _stats.remove(id);
    await _storage.saveForwards(_forwards);
    _logService.info(config.name, 'Tunnel removed');
    notifyListeners();
  }

  /// Moves a tunnel from [oldIndex] to [newIndex] and persists the new order.
  /// Order is stored implicitly as the position in the forwards list, so saving
  /// the reordered list is all that's needed for it to survive app restarts.
  Future<void> reorderForward(int oldIndex, int newIndex) async {
    if (oldIndex < 0 || oldIndex >= _forwards.length) return;
    // ReorderableListView reports newIndex assuming the item is still present;
    // adjust when moving an item further down the list.
    if (newIndex > oldIndex) newIndex -= 1;
    if (newIndex < 0) newIndex = 0;
    if (newIndex >= _forwards.length) newIndex = _forwards.length - 1;
    if (newIndex == oldIndex) return;

    final item = _forwards.removeAt(oldIndex);
    _forwards.insert(newIndex, item);
    // Notify synchronously so the list reflects the new order on the same frame
    // ReorderableListView settles the drop — awaiting the disk write first lets
    // the list snap back to the old order for a frame (the post-drop glitch).
    notifyListeners();
    await _storage.saveForwards(_forwards);
  }

  Future<void> duplicateForward(String id) async {
    final original = _forwards.firstWhere((f) => f.id == id);
    final duplicate = original.copyWith(
      id: const Uuid().v4(),
      name: '${original.name} (copy)',
    );
    _forwards.add(duplicate);
    await _storage.saveForwards(_forwards);
    notifyListeners();
  }

  Future<void> toggleForward(String id) async {
    final status = getStatus(id);
    final isReconnecting = _reconnectTimers.containsKey(id);

    // Ignore clicks during transient disconnecting state
    if (status == ForwardStatus.disconnecting) return;

    // Error state: user wants to retry — reconnect, not disconnect
    if (status == ForwardStatus.error) {
      _userDisconnected.remove(id);
      _cancelReconnect(id);
      _reconnectAttempts.remove(id);

      _statuses[id] = ForwardStatus.connecting;
      _errorMessages.remove(id);
      _stats.remove(id);
      notifyListeners();

      await _tunnel.disconnect(id);
      final config = _forwards.firstWhere((f) => f.id == id);
      await _waitForPortAvailable(config.localBindAddress, config.localPort);
      await _connectForward(id);
      return;
    }

    // Active or pending reconnect: force disconnect
    if (status == ForwardStatus.connected ||
        status == ForwardStatus.connecting ||
        isReconnecting) {
      _userDisconnected.add(id);
      _cancelReconnect(id);
      await _disconnectForward(id);
    } else {
      // Strictly disconnected — start it
      _userDisconnected.remove(id);
      _cancelReconnect(id);
      _reconnectAttempts.remove(id);

      _statuses[id] = ForwardStatus.connecting;
      _errorMessages.remove(id);
      _stats.remove(id);
      notifyListeners();

      await _tunnel.disconnect(id);
      final config = _forwards.firstWhere((f) => f.id == id);
      await _waitForPortAvailable(config.localBindAddress, config.localPort);
      await _connectForward(id);
    }
  }

  Future<void> _waitForPortAvailable(String address, int port,
      {int maxAttempts = 15}) async {
    if (skipPortWait) return;
    for (var i = 0; i < maxAttempts; i++) {
      try {
        // Try to bind with shared: false to ensure we can actually own it exclusively if needed,
        // then immediately release it.
        final socket = await ServerSocket.bind(address, port, shared: false);
        await socket.close();
        // Give the OS a tiny bit of time to actually put it in TIME_WAIT or release it
        await Future.delayed(const Duration(milliseconds: 50));
        return;
      } catch (_) {
        await Future.delayed(const Duration(milliseconds: 200));
      }
    }
  }

  Future<void> _connectForward(String id) async {
    final config = _forwards.firstWhere((f) => f.id == id);

    if (config.needsPassword) {
      _statuses[id] = ForwardStatus.error;
      _errorMessages[id] = 'Password or identity file required';
      _logService.error(config.name, 'Password or identity file required');
      notifyListeners();
      return;
    }

    _logService.info(config.name, 'Connecting to ${config.sshHost}:${config.sshPort}...');

    await _tunnel.connect(
      config,
      onStatusChanged: (id, status, errorMessage) {
        _statuses[id] = status;
        if (errorMessage != null) {
          _errorMessages[id] = errorMessage;
        } else {
          _errorMessages.remove(id);
        }
        notifyListeners();

        switch (status) {
          case ForwardStatus.connected:
            _reconnectAttempts.remove(id);
            _stats[id] = _tunnel.getStats(id) ?? const TunnelStats();
            _logService.info(config.name,
                'Connected (:${config.localPort} -> ${config.remoteHost}:${config.remotePort})');
            if (_notificationsEnabled) {
              _notification.showConnected(config.name);
            }
          case ForwardStatus.disconnected:
            // Unexpected disconnect (SSH died on its own). User-initiated
            // disconnects go through _disconnectForward() which is silent.
            _stats.remove(id);
            _logService.info(config.name, 'Disconnected');
            if (_notificationsEnabled) {
              _notification.showDisconnected(config.name);
            }
            _tryAutoReconnect(id);
          case ForwardStatus.error:
            _stats.remove(id);
            _logService.error(config.name, errorMessage ?? 'Unknown error');
            _tryAutoReconnect(id);
            // Notify only when no retry timer was scheduled — covers all
            // "error is final" cases: disabled auto-reconnect, user-disconnected,
            // and retries exhausted. Avoids duplicating the guard logic here.
            if (!_reconnectTimers.containsKey(id) && _notificationsEnabled) {
              _notification.showError(config.name, errorMessage ?? 'Unknown');
            }
          case ForwardStatus.connecting:
          case ForwardStatus.disconnecting:
            break;
        }
      },
    );
  }

  void _tryAutoReconnect(String id) {
    if (!_autoReconnect) return;
    if (_userDisconnected.contains(id)) return;
    // Guard: tunnel may have been removed while a reconnect was in flight.
    if (!_forwards.any((f) => f.id == id)) return;

    final attempts = _reconnectAttempts[id] ?? 0;
    final config = _forwards.firstWhere((f) => f.id == id);

    if (attempts >= _autoReconnectMaxRetries) {
      _logService.warning(config.name,
          'Auto-reconnect failed after $attempts attempts');
      // No timer scheduled — caller's notification guard will fire.
      return;
    }

    _reconnectAttempts[id] = attempts + 1;

    final delay = (_autoReconnectDelaySec * (1 << attempts)).clamp(1, 60);

    _logService.info(config.name,
        'Auto-reconnecting in ${delay}s (attempt ${attempts + 1}/$_autoReconnectMaxRetries)...');

    _reconnectTimers[id]?.cancel();
    _reconnectTimers[id] = Timer(
      Duration(seconds: delay),
      () {
        _reconnectTimers.remove(id);
        if (!_forwards.any((f) => f.id == id)) return;
        if (_userDisconnected.contains(id)) return;

        // Show connecting immediately so the UI reflects the retry in progress.
        _statuses[id] = ForwardStatus.connecting;
        _errorMessages.remove(id);
        notifyListeners();

        _connectForward(id);
      },
    );
  }

  void _cancelReconnect(String id) {
    _reconnectTimers[id]?.cancel();
    _reconnectTimers.remove(id);
    _reconnectAttempts.remove(id);
  }

  Future<void> _disconnectForward(String id) async {
    final config = _forwards.firstWhere((f) => f.id == id);
    _statuses[id] = ForwardStatus.disconnecting;
    _errorMessages.remove(id);
    notifyListeners();

    await _tunnel.disconnect(id);
    _statuses[id] = ForwardStatus.disconnected;
    _stats.remove(id);
    _logService.info(config.name, 'Disconnected');
    notifyListeners();
    // No notification — user-initiated disconnect is intentional and silent.
  }

  Future<void> connectAll() async {
    final toConnect = _forwards.where((f) {
      final status = getStatus(f.id);
      return status == ForwardStatus.disconnected ||
          status == ForwardStatus.error;
    }).toList();

    for (final f in toConnect) {
      _userDisconnected.remove(f.id);
      _cancelReconnect(f.id);
      _reconnectAttempts.remove(f.id);
      _statuses[f.id] = ForwardStatus.connecting; // Set to connecting immediately
      _errorMessages.remove(f.id);
      _stats.remove(f.id);
    }
    notifyListeners();

    await Future.wait(toConnect.map((f) async {
      await _tunnel.disconnect(f.id);
      await _waitForPortAvailable(f.localBindAddress, f.localPort);
      await _connectForward(f.id);
    }));
  }

  Future<void> disconnectAll() async {
    for (final f in _forwards) {
      _userDisconnected.add(f.id);
      _cancelReconnect(f.id);
      final status = getStatus(f.id);
      if (status == ForwardStatus.connected ||
          status == ForwardStatus.connecting) {
        _statuses[f.id] = ForwardStatus.disconnecting;
      }
    }
    notifyListeners();

    await _tunnel.disconnectAll();
    _statuses.clear();
    _errorMessages.clear();
    _stats.clear();
    notifyListeners();
  }

  Future<void> checkAndReconnectAll() async {
    final connectedIds = _forwards
        .where((f) => _statuses[f.id] == ForwardStatus.connected)
        .map((f) => f.id)
        .toList();

    if (connectedIds.isEmpty) return;

    _logService.info('System', 'Checking ${connectedIds.length} tunnel(s) after wake...');

    final results = await Future.wait(connectedIds.map((id) async {
      final alive = await _tunnel.isAlive(id);
      return (id: id, alive: alive);
    }));

    final dead = results.where((r) => !r.alive).map((r) => r.id).toList();
    if (dead.isEmpty) return;

    await Future.wait(dead.map((id) async {
      final config = _forwards.firstWhere((f) => f.id == id);
      _logService.warning(config.name, 'Connection lost after sleep, reconnecting...');
      _statuses[id] = ForwardStatus.disconnecting;
      notifyListeners();
      await _tunnel.disconnect(id);
      _statuses[id] = ForwardStatus.disconnected;
      _errorMessages.remove(id);
      _stats.remove(id);
      notifyListeners();
      await _waitForPortAvailable(config.localBindAddress, config.localPort);
      _reconnectAttempts.remove(id);
      await _connectForward(id);
    }));
  }

  Future<void> exportBackup(String path) async {
    await _storage.exportToFile(path, _forwards);
  }

  Future<List<ForwardConfig>> importBackup(String path) async {
    final imported = await _storage.importFromFile(path);
    _forwards = imported;
    await _storage.saveForwards(_forwards);
    notifyListeners();
    return imported;
  }

  // ── Test helpers (only used in tests) ─────────────────────────────────────

  /// Skip the real socket bind check in unit tests.
  @visibleForTesting
  bool skipPortWait = false;

  @visibleForTesting
  void forceStatus(String id, ForwardStatus status) {
    _statuses[id] = status;
    notifyListeners();
  }

  @visibleForTesting
  void triggerAutoReconnect(String id) {
    _tryAutoReconnect(id);
  }

  /// Directly runs the reconnect timer body without waiting for the delay.
  /// Use this in tests that need to verify the post-timer state.
  @visibleForTesting
  Future<void> fireReconnectNow(String id) async {
    _reconnectTimers[id]?.cancel();
    _reconnectTimers.remove(id);
    if (!_forwards.any((f) => f.id == id)) return;
    if (_userDisconnected.contains(id)) return;
    _statuses[id] = ForwardStatus.connecting;
    _errorMessages.remove(id);
    notifyListeners();
    await _connectForward(id);
  }
}
