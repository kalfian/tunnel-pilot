import 'dart:io';

import 'package:system_tray/system_tray.dart';

import '../models/forward_config.dart';
import '../models/forward_status.dart';

class TrayService {
  final SystemTray _systemTray = SystemTray();
  final void Function() onSettingsClicked;
  final void Function() onQuitClicked;
  final void Function(String id) onToggleForward;
  final void Function()? onUpdateClicked;
  final void Function()? onConnectAll;
  final void Function()? onDisconnectAll;

  TrayService({
    required this.onSettingsClicked,
    required this.onQuitClicked,
    required this.onToggleForward,
    this.onUpdateClicked,
    this.onConnectAll,
    this.onDisconnectAll,
  });

  Future<void> init() async {
    final iconPath = _iconPath(0);

    await _systemTray.initSystemTray(
      title: '',
      iconPath: iconPath,
      toolTip: 'Tunnel Pilot - SSH Port Forwarding',
    );

    _systemTray.registerSystemTrayEventHandler((eventName) {
      if (eventName == kSystemTrayEventClick ||
          eventName == kSystemTrayEventRightClick) {
        _systemTray.popUpContextMenu();
      }
    });

    await rebuildMenu([], {});
  }

  String _iconPath(int connectedCount) {
    final ext = Platform.isWindows ? 'ico' : 'png';
    final name = connectedCount <= 0
        ? 'tray_icon_idle'
        : 'tray_icon_${connectedCount.clamp(1, 9)}';
    return _assetPath('$name.$ext');
  }

  String _statusImagePath(ForwardStatus status) {
    switch (status) {
      case ForwardStatus.connected:
        return _assetPath('status_green.png');
      case ForwardStatus.connecting:
        return _assetPath('status_yellow.png');
      case ForwardStatus.error:
        return _assetPath('status_red.png');
      case ForwardStatus.disconnected:
        return _assetPath('status_grey.png');
    }
  }

  String _assetPath(String filename) {
    final assetRelPath = 'assets/icons/$filename';

    // macOS resolves bundle-relative paths automatically
    if (Platform.isMacOS) return assetRelPath;

    // Windows/Linux: system_tray needs an absolute path.
    final execDir = File(Platform.resolvedExecutable).parent.path;
    final sep = Platform.pathSeparator;
    return '$execDir${sep}data${sep}flutter_assets${sep}assets${sep}icons${sep}$filename';
  }

  Future<void> rebuildMenu(
    List<ForwardConfig> forwards,
    Map<String, ForwardStatus> statuses, {
    bool updateAvailable = false,
    String? latestVersion,
  }) async {
    // Count active connections
    final connectedCount = statuses.values
        .where((s) => s == ForwardStatus.connected)
        .length;

    // Update tray icon based on connection count
    await _systemTray.setImage(_iconPath(connectedCount));
    await _systemTray.setToolTip(connectedCount > 0
        ? 'Tunnel Pilot - $connectedCount active'
        : 'Tunnel Pilot - No active tunnels');

    final List<MenuItemBase> menuItems = [];

    // ── Header ──
    menuItems.add(MenuItemLabel(
      label: 'Tunnel Pilot',
      enabled: false,
    ));

    if (connectedCount > 0) {
      menuItems.add(MenuItemLabel(
        label: '$connectedCount tunnel${connectedCount > 1 ? 's' : ''} active',
        enabled: false,
      ));
    }

    menuItems.add(MenuSeparator());

    // ── Update notice ──
    if (updateAvailable && latestVersion != null) {
      menuItems.add(MenuItemLabel(
        label: 'Update available (v$latestVersion)',
        onClicked: (_) {
          if (onUpdateClicked != null) {
            onUpdateClicked!();
          } else {
            onSettingsClicked();
          }
        },
      ));
      menuItems.add(MenuSeparator());
    }

    // ── Tunnels ──
    if (forwards.isEmpty) {
      menuItems.add(MenuItemLabel(
        label: 'No tunnels configured',
        enabled: false,
      ));
    } else {
      for (final forward in forwards) {
        final status = statuses[forward.id] ?? ForwardStatus.disconnected;
        final portInfo = ':${forward.localPort} → :${forward.remotePort}';
        menuItems.add(MenuItemLabel(
          label: '${forward.name}  ($portInfo)',
          image: _statusImagePath(status),
          onClicked: (_) => onToggleForward(forward.id),
        ));
      }

      menuItems.add(MenuSeparator());

      // ── Connect All / Disconnect All ──
      final hasDisconnected = statuses.values.any((s) =>
          s == ForwardStatus.disconnected || s == ForwardStatus.error);
      final hasConnected = statuses.values.any((s) =>
          s == ForwardStatus.connected || s == ForwardStatus.connecting);

      if (hasDisconnected && onConnectAll != null) {
        menuItems.add(MenuItemLabel(
          label: 'Connect All',
          onClicked: (_) => onConnectAll!(),
        ));
      }
      if (hasConnected && onDisconnectAll != null) {
        menuItems.add(MenuItemLabel(
          label: 'Disconnect All',
          onClicked: (_) => onDisconnectAll!(),
        ));
      }
    }

    menuItems.add(MenuSeparator());

    // ── Footer ──
    menuItems.add(MenuItemLabel(
      label: 'Settings...',
      onClicked: (_) => onSettingsClicked(),
    ));

    menuItems.add(MenuItemLabel(
      label: 'Quit',
      onClicked: (_) => onQuitClicked(),
    ));

    final menu = Menu();
    await menu.buildFrom(menuItems);
    await _systemTray.setContextMenu(menu);
  }
}
