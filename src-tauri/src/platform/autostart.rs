//! Launch-at-login via `tauri-plugin-autostart`; reconcile with `launchAtLogin`
//! on every boot; autostarted launches open hidden (spec 03 §12, 06 F18).
//!
//! The plugin is registered in `lib.rs` (as a `LaunchAgent` on macOS). On boot
//! we read `settings.launch_at_login` and drive the OS registration to match, so
//! any drift (user toggled it in System Settings, or a stale registration) is
//! corrected. `update_settings` (M4) calls [`reconcile`] again on change.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// Reconcile the OS autostart registration with the desired `launch_at_login`
/// state. Idempotent: only enables/disables when the current state differs.
pub fn reconcile(app: &AppHandle, launch_at_login: bool) {
    let manager = app.autolaunch();
    let currently_enabled = match manager.is_enabled() {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "failed to query autostart state; skipping reconcile");
            return;
        }
    };

    if launch_at_login && !currently_enabled {
        if let Err(e) = manager.enable() {
            tracing::error!(error = %e, "failed to enable autostart");
        } else {
            tracing::info!("autostart enabled to match setting");
        }
    } else if !launch_at_login && currently_enabled {
        if let Err(e) = manager.disable() {
            tracing::error!(error = %e, "failed to disable autostart");
        } else {
            tracing::info!("autostart disabled to match setting");
        }
    }
}
