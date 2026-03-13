import Cocoa
import FlutterMacOS
import ServiceManagement
import UserNotifications

@main
class AppDelegate: FlutterAppDelegate, UNUserNotificationCenterDelegate {
  override func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
    // Keep app alive when window is hidden — it lives in the system tray
    return false
  }

  override func applicationSupportsSecureRestorableState(_ app: NSApplication) -> Bool {
    return true
  }

  override func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
    // User re-launched the .app → tell Flutter to show the settings window
    if let controller = mainFlutterWindow?.contentViewController as? FlutterViewController {
      let channel = FlutterMethodChannel(name: "app_lifecycle", binaryMessenger: controller.engine.binaryMessenger)
      channel.invokeMethod("showSettings", arguments: nil)
    }
    return true
  }

  override func applicationDidFinishLaunching(_ notification: Notification) {
    // Set up UNUserNotificationCenter delegate BEFORE changing activation policy
    let center = UNUserNotificationCenter.current()
    center.delegate = self

    // Request notification permission while still .regular (before switching to .accessory)
    // This ensures the permission dialog can be shown to the user
    center.getNotificationSettings { settings in
      if settings.authorizationStatus == .notDetermined {
        // App is still .regular at this point, so the dialog will appear
        DispatchQueue.main.async {
          NSApp.activate(ignoringOtherApps: true)
        }
        center.requestAuthorization(options: [.alert, .sound, .badge]) { granted, error in
          if let error = error {
            print("[Notifications] Permission error: \(error.localizedDescription)")
          }
          print("[Notifications] Permission granted: \(granted)")
          // Now switch to accessory after permission is handled
          DispatchQueue.main.async {
            NSApp.setActivationPolicy(.accessory)
          }
        }
      } else {
        print("[Notifications] Already determined (status: \(settings.authorizationStatus.rawValue))")
        // Already determined, go to accessory immediately
        DispatchQueue.main.async {
          NSApp.setActivationPolicy(.accessory)
        }
      }
    }

    let controller = mainFlutterWindow?.contentViewController as! FlutterViewController

    // Launch at startup channel
    let launchChannel = FlutterMethodChannel(name: "launch_at_startup", binaryMessenger: controller.engine.binaryMessenger)

    launchChannel.setMethodCallHandler { (call, result) in
      if #available(macOS 13.0, *) {
        switch call.method {
        case "launchAtStartupIsEnabled":
          let enabled = SMAppService.mainApp.status == .enabled
          result(enabled)
        case "launchAtStartupSetEnabled":
          guard let args = call.arguments as? [String: Any],
                let setEnabled = args["setEnabledValue"] as? Bool else {
            result(FlutterError(code: "INVALID_ARGS", message: "Missing setEnabledValue", details: nil))
            return
          }
          do {
            if setEnabled {
              try SMAppService.mainApp.register()
            } else {
              try SMAppService.mainApp.unregister()
            }
            result(true)
          } catch {
            result(FlutterError(code: "SM_ERROR", message: error.localizedDescription, details: nil))
          }
        default:
          result(FlutterMethodNotImplemented)
        }
      } else {
        // macOS < 13 fallback using deprecated SMLoginItemSetEnabled
        switch call.method {
        case "launchAtStartupIsEnabled":
          result(false)
        case "launchAtStartupSetEnabled":
          result(false)
        default:
          result(FlutterMethodNotImplemented)
        }
      }
    }

    // Native notifications channel (UNUserNotificationCenter — modern API)
    let notifChannel = FlutterMethodChannel(name: "native_notifications", binaryMessenger: controller.engine.binaryMessenger)

    notifChannel.setMethodCallHandler { (call, result) in
      switch call.method {
      case "show":
        guard let args = call.arguments as? [String: Any],
              let title = args["title"] as? String,
              let body = args["body"] as? String else {
          result(FlutterError(code: "INVALID_ARGS", message: "Missing title or body", details: nil))
          return
        }
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        content.sound = .default
        let request = UNNotificationRequest(
          identifier: UUID().uuidString,
          content: content,
          trigger: nil
        )
        UNUserNotificationCenter.current().add(request) { error in
          if let error = error {
            print("[Notifications] Send error: \(error.localizedDescription)")
            result(FlutterError(code: "NOTIF_ERROR", message: error.localizedDescription, details: nil))
          } else {
            result(true)
          }
        }
      default:
        result(FlutterMethodNotImplemented)
      }
    }
  }

  // Show notification banner even when app is in foreground
  func userNotificationCenter(
    _ center: UNUserNotificationCenter,
    willPresent notification: UNNotification,
    withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
  ) {
    if #available(macOS 11.0, *) {
      completionHandler([.banner, .sound])
    } else {
      completionHandler([.alert, .sound])
    }
  }
}
