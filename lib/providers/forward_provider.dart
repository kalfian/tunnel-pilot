import 'package:flutter/foundation.dart';
import 'package:uuid/uuid.dart';

import '../models/forward_config.dart';
import '../models/forward_status.dart';
import '../services/notification_service.dart';
import '../services/ssh_tunnel_service.dart';
import '../services/storage_service.dart';

class ForwardProvider extends ChangeNotifier {
  final StorageService _storage;
  final SshTunnelService _tunnel;
  final NotificationService _notification;
  bool _notificationsEnabled;

  List<ForwardConfig> _forwards = [];
  final Map<String, ForwardStatus> _statuses = {};
  final Map<String, String> _errorMessages = {};

  ForwardProvider({
    required StorageService storage,
    required SshTunnelService tunnel,
    required NotificationService notification,
    bool notificationsEnabled = true,
  })  : _storage = storage,
        _tunnel = tunnel,
        _notification = notification,
        _notificationsEnabled = notificationsEnabled;

  List<ForwardConfig> get forwards => List.unmodifiable(_forwards);

  ForwardStatus getStatus(String id) =>
      _statuses[id] ?? ForwardStatus.disconnected;

  String? getErrorMessage(String id) => _errorMessages[id];

  set notificationsEnabled(bool value) => _notificationsEnabled = value;

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
    }

    _forwards[index] = config;
    await _storage.saveForwards(_forwards);
    notifyListeners();
  }

  Future<void> removeForward(String id) async {
    await _tunnel.disconnect(id);
    _forwards.removeWhere((f) => f.id == id);
    _statuses.remove(id);
    _errorMessages.remove(id);
    await _storage.saveForwards(_forwards);
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
      await _disconnectForward(id);
    } else {
      await _connectForward(id);
    }
  }

  Future<void> _connectForward(String id) async {
    final config = _forwards.firstWhere((f) => f.id == id);

    if (config.needsPassword) {
      _statuses[id] = ForwardStatus.error;
      _errorMessages[id] = 'Password or identity file required';
      notifyListeners();
      return;
    }

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

        if (!_notificationsEnabled) return;

        switch (status) {
          case ForwardStatus.connected:
            _notification.showConnected(config.name);
          case ForwardStatus.disconnected:
            _notification.showDisconnected(config.name);
          case ForwardStatus.error:
            _notification.showError(config.name, errorMessage ?? 'Unknown');
          case ForwardStatus.connecting:
            break;
        }
      },
    );
  }

  Future<void> _disconnectForward(String id) async {
    final config = _forwards.firstWhere((f) => f.id == id);
    await _tunnel.disconnect(id);
    _statuses[id] = ForwardStatus.disconnected;
    _errorMessages.remove(id);
    notifyListeners();

    if (_notificationsEnabled) {
      _notification.showDisconnected(config.name);
    }
  }

  Future<void> disconnectAll() async {
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
