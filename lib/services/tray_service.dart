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

  TrayService({
    required this.onSettingsClicked,
    required this.onQuitClicked,
    required this.onToggleForward,
    this.onUpdateClicked,
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
    final assetRelPath = 'assets/icons/$name.$ext';

    // macOS resolves bundle-relative paths automatically
    if (Platform.isMacOS) return assetRelPath;

    // Windows/Linux: system_tray needs an absolute path.
    // Flutter bundles assets at {exe_dir}/data/flutter_assets/
    final execDir = File(Platform.resolvedExecutable).parent.path;
    final sep = Platform.pathSeparator;
    return '$execDir${sep}data${sep}flutter_assets${sep}assets${sep}icons${sep}$name.$ext';
  }

  String _statusIcon(ForwardStatus status) {
    switch (status) {
      case ForwardStatus.connected:
        return '\u25CF'; // ● small filled circle
      case ForwardStatus.connecting:
        return '\u25D0'; // ◐ half circle (in-progress)
      case ForwardStatus.error:
        return '\u2716'; // ✖ heavy multiplication x
      case ForwardStatus.disconnected:
        return '\u25CB'; // ○ small empty circle
    }
  }

  String _statusLabel(ForwardStatus status) {
    switch (status) {
      case ForwardStatus.connected:
        return 'Connected';
      case ForwardStatus.connecting:
        return 'Connecting…';
      case ForwardStatus.error:
        return 'Error';
      case ForwardStatus.disconnected:
        return 'Off';
    }
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

    menuItems.add(MenuItemLabel(
      label: 'Tunnel Pilot${connectedCount > 0 ? '  ($connectedCount active)' : ''}',
      enabled: false,
    ));

    menuItems.add(MenuSeparator());

    if (forwards.isEmpty) {
      menuItems.add(MenuItemLabel(
        label: 'No tunnels configured',
        enabled: false,
      ));
    } else {
      for (final forward in forwards) {
        final status = statuses[forward.id] ?? ForwardStatus.disconnected;
        final icon = _statusIcon(status);
        final label = _statusLabel(status);
        menuItems.add(MenuItemLabel(
          label: '$icon ${forward.name} — $label',
          onClicked: (_) => onToggleForward(forward.id),
        ));
      }
    }

    menuItems.add(MenuSeparator());

    if (updateAvailable && latestVersion != null) {
      menuItems.add(MenuItemLabel(
        label: '\u2B06\uFE0F  Update available (v$latestVersion)',
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

    menuItems.add(MenuItemLabel(
      label: 'Settings...',
      onClicked: (_) => onSettingsClicked(),
    ));

    menuItems.add(MenuSeparator());

    menuItems.add(MenuItemLabel(
      label: 'Quit',
      onClicked: (_) => onQuitClicked(),
    ));

    final menu = Menu();
    await menu.buildFrom(menuItems);
    await _systemTray.setContextMenu(menu);
  }
}
