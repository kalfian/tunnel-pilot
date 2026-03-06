import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import 'package:window_manager/window_manager.dart';

import 'app.dart';
import 'providers/app_settings_provider.dart';
import 'providers/forward_provider.dart';
import 'services/notification_service.dart';
import 'services/ssh_tunnel_service.dart';
import 'services/startup_service.dart';
import 'services/storage_service.dart';
import 'services/tray_service.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  await windowManager.ensureInitialized();

  const windowOptions = WindowOptions(
    size: Size(700, 600),
    minimumSize: Size(500, 400),
    center: true,
    title: 'Tunnel Pilot - Settings',
    skipTaskbar: false,
  );

  await windowManager.waitUntilReadyToShow(windowOptions, () async {
    await windowManager.setPreventClose(true);
    // Show window in debug, hide in release
    if (kReleaseMode) {
      await windowManager.hide();
    } else {
      await windowManager.show();
      await windowManager.focus();
    }
  });

  final storageService = StorageService();
  final sshTunnelService = SshTunnelService();
  final notificationService = NotificationService();
  final startupService = StartupService();

  try {
    await notificationService.init();
  } catch (e) {
    debugPrint('NotificationService init failed: $e');
  }

  try {
    final execPath = Platform.resolvedExecutable;
    startupService.init('Tunnel Pilot', execPath);
  } catch (e) {
    debugPrint('StartupService init failed: $e');
  }

  final data = await storageService.load();

  final forwardProvider = ForwardProvider(
    storage: storageService,
    tunnel: sshTunnelService,
    notification: notificationService,
    notificationsEnabled: data.settings.showNotifications,
  );
  await forwardProvider.loadForwards(data.forwards);

  final appSettingsProvider = AppSettingsProvider(
    storage: storageService,
    startup: startupService,
    settings: data.settings,
  );

  TrayService? trayService;
  try {
    trayService = TrayService(
      onSettingsClicked: () async {
        await windowManager.show();
        await windowManager.focus();
      },
      onQuitClicked: () async {
        await forwardProvider.disconnectAll();
        exit(0);
      },
      onToggleForward: (id) {
        forwardProvider.toggleForward(id);
      },
    );

    await trayService.init();
  } catch (e) {
    debugPrint('TrayService init failed: $e');
    trayService = null;
  }

  if (trayService != null) {
    forwardProvider.addListener(() {
      trayService!.rebuildMenu(
        forwardProvider.forwards,
        {
          for (final f in forwardProvider.forwards)
            f.id: forwardProvider.getStatus(f.id),
        },
      );
    });

    await trayService.rebuildMenu(
      forwardProvider.forwards,
      {
        for (final f in forwardProvider.forwards)
          f.id: forwardProvider.getStatus(f.id),
      },
    );
  }

  appSettingsProvider.addListener(() {
    forwardProvider.notificationsEnabled =
        appSettingsProvider.showNotifications;
  });

  runApp(
    MultiProvider(
      providers: [
        ChangeNotifierProvider.value(value: forwardProvider),
        ChangeNotifierProvider.value(value: appSettingsProvider),
      ],
      child: _AppWithWindowListener(
        child: const TunnelPilotApp(),
      ),
    ),
  );
}

class _AppWithWindowListener extends StatefulWidget {
  final Widget child;
  const _AppWithWindowListener({required this.child});

  @override
  State<_AppWithWindowListener> createState() => _AppWithWindowListenerState();
}

class _AppWithWindowListenerState extends State<_AppWithWindowListener>
    with WindowListener {
  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
  }

  @override
  void dispose() {
    windowManager.removeListener(this);
    super.dispose();
  }

  @override
  void onWindowClose() async {
    await windowManager.hide();
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
