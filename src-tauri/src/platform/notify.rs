//! Desktop notifications via `tauri-plugin-notification` (spec 03 §15).
//!
//! Rules (replicate v1 `notification_service.dart` + `forward_provider.dart`):
//! - Notify on **connect** and on **terminal error** (unexpected states).
//! - **User-initiated disconnects are SILENT.** In the v2 state machine an
//!   unexpected drop transitions `connected → error` (never `→ disconnected`),
//!   and `disconnected` is written ONLY by the user-disconnect command handler
//!   (spec 03 §1 transition table). So "user-initiated disconnect is silent" is
//!   structurally satisfied by simply *not* notifying on `disconnected` — there
//!   is no unexpected `disconnected` path to distinguish. The teardown decision
//!   already carries `user_initiated`; this module never fires for it.
//! - A transient `error` that will auto-reconnect is NOT notified — only the
//!   *terminal* error (retries exhausted / auto-reconnect off) is, matching v1's
//!   "notify only when no retry was scheduled" (`forward_provider.dart:274`).
//! - Update-available notifies **once per version** (dedup lives in the updater
//!   module, spec 03 §16).
//! - All of the above honor the `showNotifications` setting.
//!
//! **macOS permission timing.** The desktop `tauri-plugin-notification` reports
//! permission as always-`Granted` and has no meaningful `request_permission`
//! (it is a no-op on desktop); the real macOS `UNUserNotificationCenter` prompt,
//! if any, is triggered lazily by the first `show()`. We never call `show()` at
//! startup — only on a real connect/error/update event — so the permission
//! interaction happens at a deliberate, user-triggered moment, not a boot race.
//!
//! **F5 — unsigned macOS caveat.** macOS delivers notifications through
//! `UNUserNotificationCenter`, which generally requires a **code-signed /
//! registered** bundle. v2.0 ships un-notarized/unsigned (spec 01 §3.3, 06 §4),
//! so `show()` may **silently drop** on a bundled macOS app: the underlying
//! `notify_rust`/`mac-notification-sys` call sets the app to the bundle
//! identifier (`com.kalfian.tunnelpilot`), which is unregistered with the
//! notification system on an unsigned build, and returns `Ok(())` even when
//! nothing is displayed. We therefore treat every notification as **best-effort**
//! and rely on the tray icon state + in-window log/status as the authoritative
//! signal. Never panic, never propagate — a failed notification is logged at
//! `debug` and ignored.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;

/// Fire a best-effort desktop notification. Errors are swallowed (logged at
/// `debug`) because notifications are advisory — the tray + log are the source
/// of truth (F5). No-op paths are handled by the callers below.
fn show(app: &AppHandle, title: &str, body: &str) {
    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => {
            tracing::debug!(title, "notification shown (best-effort)");
        }
        Err(e) => {
            // Expected on unsigned macOS bundles (F5) and headless Linux without
            // a notification daemon. Tray/log remain authoritative.
            tracing::debug!(error = %e, title, "notification not shown (best-effort; tray/log authoritative)");
        }
    }
}

/// Whether notifications are enabled AND we have a live app handle (headless
/// engine tests have neither an app handle nor a real notification backend).
fn target(state: &AppState) -> Option<AppHandle> {
    if !state.settings_snapshot().show_notifications {
        return None;
    }
    state.app_handle()
}

/// Notify that a tunnel came up (user connect OR successful auto-reconnect),
/// matching v1's `showConnected`.
pub fn notify_connected(state: &AppState, name: &str) {
    if let Some(app) = target(state) {
        show(&app, "Tunnel Connected", &format!("{name} is now active."));
    }
}

/// Notify a **terminal** tunnel error (auto-reconnect exhausted or disabled),
/// matching v1's `showError` fired only when no retry was scheduled. Callers
/// MUST NOT call this for a transient error that will auto-reconnect.
pub fn notify_error(state: &AppState, name: &str, error: &str) {
    if let Some(app) = target(state) {
        show(&app, "Tunnel Error", &format!("{name}: {error}"));
    }
}

/// Notify that an app update is available. Once-per-version dedup is the
/// caller's responsibility (updater module tracks `last_notified_version`).
pub fn notify_update_available(state: &AppState, version: &str) {
    if let Some(app) = target(state) {
        show(
            &app,
            "Update Available",
            &format!("Tunnel Pilot {version} is available. Open Tunnel Pilot to update."),
        );
    }
}
