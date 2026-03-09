import Cocoa
import FlutterMacOS
import ServiceManagement

@main
class AppDelegate: FlutterAppDelegate {
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
    // Hide from Dock — run as accessory (tray-only) app
    NSApp.setActivationPolicy(.accessory)

    let controller = mainFlutterWindow?.contentViewController as! FlutterViewController
    let channel = FlutterMethodChannel(name: "launch_at_startup", binaryMessenger: controller.engine.binaryMessenger)

    channel.setMethodCallHandler { (call, result) in
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
  }
}
