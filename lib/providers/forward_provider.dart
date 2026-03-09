import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:uuid/uuid.dart';

import '../models/forward_config.dart';
import '../models/forward_status.dart';
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
  // Track which tunnels the user explicitly disconnected
  final Set<String> _userDisconnected = {};

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
        _autoReconnectMaxRetries = autoReconnectMaxRetries;

  List<ForwardConfig> get forwards => List.unmodifiable(_forwards);

  ForwardStatus getStatus(String id) =>
      _statuses[id] ?? ForwardStatus.disconnected;

  String? getErrorMessage(String id) => _errorMessages[id];

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
      await _tunnel.disconnect(config.id);
      _statuses[config.id] = ForwardStatus.disconnected;
      _logService.info(config.name, 'Disconnected (config updated)');
    }

    _forwards[index] = config;
    await _storage.saveForwards(_forwards);
    notifyListeners();
  }

  Future<void> removeForward(String id) async {
    final config = _forwards.firstWhere((f) => f.id == id);
    _cancelReconnect(id);
    await _tunnel.disconnect(id);
    _forwards.removeWhere((f) => f.id == id);
    _statuses.remove(id);
    _errorMessages.remove(id);
    _userDisconnected.remove(id);
    await _storage.saveForwards(_forwards);
    _logService.info(config.name, 'Tunnel removed');
    notifyListeners();
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
    if (status == ForwardStatus.connected ||
        status == ForwardStatus.connecting) {
      _userDisconnected.add(id);
      _cancelReconnect(id);
      await _disconnectForward(id);
    } else {
      _userDisconnected.remove(id);
      _cancelReconnect(id);
      _reconnectAttempts.remove(id);
      // Force to disconnected first to clear any stale error/connecting state
      await _tunnel.disconnect(id);
      _statuses[id] = ForwardStatus.disconnected;
      _errorMessages.remove(id);
      notifyListeners();
      // Small delay to let OS release the local port (TIME_WAIT)
      await Future.delayed(const Duration(milliseconds: 500));
      await _connectForward(id);
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
            _logService.info(config.name,
                'Connected (:${config.localPort} -> ${config.remoteHost}:${config.remotePort})');
            if (_notificationsEnabled) {
              _notification.showConnected(config.name);
            }
          case ForwardStatus.disconnected:
            _logService.info(config.name, 'Disconnected');
            if (_notificationsEnabled) {
              _notification.showDisconnected(config.name);
            }
            _tryAutoReconnect(id);
          case ForwardStatus.error:
            _logService.error(config.name, errorMessage ?? 'Unknown error');
            if (_notificationsEnabled) {
              _notification.showError(config.name, errorMessage ?? 'Unknown');
            }
            _tryAutoReconnect(id);
          case ForwardStatus.connecting:
            break;
        }
      },
    );
  }

  void _tryAutoReconnect(String id) {
    if (!_autoReconnect) return;
    if (_userDisconnected.contains(id)) return;

    final attempts = _reconnectAttempts[id] ?? 0;
    if (attempts >= _autoReconnectMaxRetries) {
      final config = _forwards.firstWhere((f) => f.id == id);
      _logService.warning(config.name,
          'Auto-reconnect failed after $attempts attempts');
      return;
    }

    _reconnectAttempts[id] = attempts + 1;
    final config = _forwards.firstWhere((f) => f.id == id);
    _logService.info(config.name,
        'Auto-reconnecting in ${_autoReconnectDelaySec}s (attempt ${attempts + 1}/$_autoReconnectMaxRetries)...');

    _reconnectTimers[id]?.cancel();
    _reconnectTimers[id] = Timer(
      Duration(seconds: _autoReconnectDelaySec),
      () {
        _reconnectTimers.remove(id);
        if (_forwards.any((f) => f.id == id) &&
            !_userDisconnected.contains(id)) {
          _connectForward(id);
        }
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
    await _tunnel.disconnect(id);
    _statuses[id] = ForwardStatus.disconnected;
    _errorMessages.remove(id);
    _logService.info(config.name, 'Disconnected');
    notifyListeners();

    if (_notificationsEnabled) {
      _notification.showDisconnected(config.name);
    }
  }

  Future<void> disconnectAll() async {
    for (final f in _forwards) {
      _userDisconnected.add(f.id);
      _cancelReconnect(f.id);
    }
    await _tunnel.disconnectAll();
    _statuses.clear();
    _errorMessages.clear();
    notifyListeners();
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
}
