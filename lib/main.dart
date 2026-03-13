import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:window_manager/window_manager.dart';

import 'app.dart';
import 'providers/app_settings_provider.dart';
import 'providers/forward_provider.dart';
import 'services/log_service.dart';
import 'services/notification_service.dart';
import 'services/ssh_tunnel_service.dart';
import 'services/startup_service.dart';
import 'services/storage_service.dart';
import 'services/single_instance_service.dart';
import 'services/tray_service.dart';
import 'services/update_service.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();

  // Single-instance guard (Windows/Linux).
  // macOS uses applicationShouldHandleReopen instead.
  final singleInstance = SingleInstanceService();
  if (!Platform.isMacOS) {
    final isPrimary = await singleInstance.ensureSingleInstance();
    if (!isPrimary) {
      exit(0);
    }
  }

  await windowManager.ensureInitialized();

  // macOS: native title bar hidden via MainFlutterWindow.swift (traffic lights removed).
  // Windows/Linux: hide via window_manager so only the custom Flutter close button shows.
  final windowOptions = WindowOptions(
    size: const Size(700, 600),
    minimumSize: const Size(700, 600),
    maximumSize: const Size(700, 600),
    center: true,
    title: 'Tunnel Pilot',
    skipTaskbar: true,
    titleBarStyle: TitleBarStyle.normal,
  );

  await windowManager.waitUntilReadyToShow(windowOptions, () async {
    await windowManager.setPreventClose(true);
    await windowManager.setResizable(false);
    // Always show first so the engine stays alive
    await windowManager.show();
  });

  final storageService = StorageService();
  final sshTunnelService = SshTunnelService();
  final notificationService = NotificationService();
  final startupService = StartupService();
  final logService = LogService();
  final updateService = UpdateService();

  try {
    await updateService.init();
  } catch (e) {
    debugPrint('UpdateService init failed: $e');
  }

  try {
    await notificationService.init();
  } catch (e) {
    debugPrint('NotificationService init failed: $e');
  }

  try {
    // launch_at_startup needs the .app bundle path, not the inner executable
    String appPath = Platform.resolvedExecutable;
    final appIndex = appPath.indexOf('.app/');
    if (appIndex != -1) {
      appPath = appPath.substring(0, appIndex + 4);
    }
    startupService.init('Tunnel Pilot', appPath);
  } catch (e) {
    debugPrint('StartupService init failed: $e');
  }

  final data = await storageService.load();

  final forwardProvider = ForwardProvider(
    storage: storageService,
    tunnel: sshTunnelService,
    notification: notificationService,
    logService: logService,
    notificationsEnabled: data.settings.showNotifications,
    autoReconnect: data.settings.autoReconnect,
    autoReconnectDelaySec: data.settings.autoReconnectDelaySec,
    autoReconnectMaxRetries: data.settings.autoReconnectMaxRetries,
  );
  await forwardProvider.loadForwards(data.forwards);

  final appSettingsProvider = AppSettingsProvider(
    storage: storageService,
    startup: startupService,
    settings: data.settings,
  );

  // Sync launch-at-login with OS on every startup so the registration
  // stays correct (especially on first launch where the default is true).
  try {
    await startupService.setEnabled(data.settings.launchAtLogin);
  } catch (e) {
    debugPrint('StartupService setEnabled failed: $e');
  }

  TrayService? trayService;
  try {
    trayService = TrayService(
      onSettingsClicked: () async {
        final isVisible = await windowManager.isVisible();
        if (isVisible) {
          await windowManager.focus();
          return;
        }
        await windowManager.setSkipTaskbar(false);
        await windowManager.show();
        await windowManager.focus();
      },
      onQuitClicked: () async {
        await forwardProvider.disconnectAll();
        await singleInstance.dispose();
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

  // Initialize update service with skipped version from settings
  updateService.setSkippedVersion(appSettingsProvider.lastSkippedVersion);

  if (trayService != null) {
    void rebuildTrayMenu() {
      trayService!.rebuildMenu(
        forwardProvider.forwards,
        {
          for (final f in forwardProvider.forwards)
            f.id: forwardProvider.getStatus(f.id),
        },
        updateAvailable: updateService.updateAvailable,
        latestVersion: updateService.latestVersion,
      );
    }

    forwardProvider.addListener(rebuildTrayMenu);
    updateService.addListener(rebuildTrayMenu);

    rebuildTrayMenu();
  }

  // Notify user when update is available (only once per version)
  String? lastNotifiedVersion;
  updateService.addListener(() {
    if (updateService.updateAvailable &&
        updateService.latestVersion != null &&
        updateService.latestVersion != lastNotifiedVersion) {
      lastNotifiedVersion = updateService.latestVersion;
      try {
        notificationService.show(
          'Update Available',
          'Tunnel Pilot v${updateService.latestVersion} is available.',
        );
      } catch (_) {}
    }
  });

  // Start auto-update check if enabled
  if (appSettingsProvider.autoCheckUpdates) {
    updateService.checkForUpdate();
    updateService.startPeriodicCheck();
  }

  // React to settings changes for auto-update
  appSettingsProvider.addListener(() {
    if (appSettingsProvider.autoCheckUpdates) {
      updateService.startPeriodicCheck();
    } else {
      updateService.stopPeriodicCheck();
    }
    updateService.setSkippedVersion(appSettingsProvider.lastSkippedVersion);
  });

  // Show settings when another instance or app reopen is detected
  Future<void> showSettings() async {
    final isVisible = await windowManager.isVisible();
    if (!isVisible) {
      if (appSettingsProvider.showInDock) {
        await windowManager.setSkipTaskbar(false);
      }
      await windowManager.show();
    }
    await windowManager.focus();
  }

  // macOS: user opens .app again → applicationShouldHandleReopen
  if (Platform.isMacOS) {
    const channel = MethodChannel('app_lifecycle');
    channel.setMethodCallHandler((call) async {
      if (call.method == 'showSettings') {
        await showSettings();
      }
    });
  } else {
    // Windows/Linux: second instance sends "show" via TCP
    singleInstance.onSecondInstance = showSettings;
  }

  appSettingsProvider.addListener(() {
    forwardProvider.notificationsEnabled =
        appSettingsProvider.showNotifications;
    forwardProvider.autoReconnect = appSettingsProvider.autoReconnect;
    forwardProvider.autoReconnectDelaySec =
        appSettingsProvider.autoReconnectDelaySec;
    forwardProvider.autoReconnectMaxRetries =
        appSettingsProvider.autoReconnectMaxRetries;
  });

  runApp(
    MultiProvider(
      providers: [
        ChangeNotifierProvider.value(value: forwardProvider),
        ChangeNotifierProvider.value(value: appSettingsProvider),
        ChangeNotifierProvider.value(value: logService),
        ChangeNotifierProvider.value(value: updateService),
      ],
      child: _AppWithWindowListener(
        appSettings: appSettingsProvider,
        child: const TunnelPilotApp(),
      ),
    ),
  );
}

class _AppWithWindowListener extends StatefulWidget {
  final Widget child;
  final AppSettingsProvider appSettings;
  const _AppWithWindowListener({required this.child, required this.appSettings});

  @override
  State<_AppWithWindowListener> createState() => _AppWithWindowListenerState();
}

class _AppWithWindowListenerState extends State<_AppWithWindowListener>
    with WindowListener, WidgetsBindingObserver {
  DateTime _lastActiveTime = DateTime.now();

  @override
  void initState() {
    super.initState();
    windowManager.addListener(this);
    WidgetsBinding.instance.addObserver(this);
    // Hide window after the first frame so the engine is fully alive
    WidgetsBinding.instance.addPostFrameCallback((_) async {
      // await windowManager.hide();
      await windowManager.setSkipTaskbar(true);
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    windowManager.removeListener(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      final now = DateTime.now();
      final elapsed = now.difference(_lastActiveTime);
      // If more than 30 seconds have passed, the system likely slept
      if (elapsed.inSeconds > 30) {
        final forwardProvider =
            Provider.of<ForwardProvider>(context, listen: false);
        forwardProvider.checkAndReconnectAll();
      }
    }
    if (state == AppLifecycleState.inactive ||
        state == AppLifecycleState.paused) {
      _lastActiveTime = DateTime.now();
    }
  }

  @override
  void onWindowClose() async {
    // Never close the window — always hide it instead.
    // The app can only be quit via "Quit" in the tray menu.
    final isPreventClose = await windowManager.isPreventClose();
    if (isPreventClose) {
      await windowManager.hide();
      if (!widget.appSettings.showInDock) {
        await windowManager.setSkipTaskbar(true);
      }
    }
  }

  @override
  Widget build(BuildContext context) => widget.child;
}
