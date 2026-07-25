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
//! **What works where (BUG 2 / F5).** macOS delivers plugin notifications
//! through `UNUserNotificationCenter`, which requires a **code-signed /
//! registered** bundle with a bundle identifier:
//!
//! | context                 | `tauri-plugin-notification` | `osascript` fallback |
//! |-------------------------|-----------------------------|----------------------|
//! | `tauri dev` (bare bin)  | silently dropped (no bundle id) | works           |
//! | bundled `.app`, UNSIGNED| may be refused (F5), returns `Ok`| works           |
//! | bundled `.app`, SIGNED  | works                       | works                |
//!
//! In `tauri dev` the binary is `target/debug/tunnel-pilot` (NOT a `.app`), so
//! the plugin has no bundle id and the OS drops the notification — and it still
//! returns `Ok(())`, so we cannot detect the drop. On an unsigned bundle the
//! native center may likewise refuse. Because the plugin cannot be relied on and
//! its silent-drop is undetectable, **macOS uses `osascript -e 'display
//! notification ...'`**, which has no bundle-id/signing requirement and works in
//! ALL three contexts. Windows/Linux use the plugin (the correct native path
//! there; Linux needs a notification daemon, a missing one just logs).
//!
//! Everything remains **best-effort**: the tray icon state + the in-window
//! log/status are the AUTHORITATIVE signal. Never panic, never propagate — a
//! failed notification is logged at `debug` and ignored. To fully verify the
//! native plugin path on macOS you need a bundled, code-signed `.app`
//! (`pnpm tauri build` + signing); the osascript path is verifiable today in
//! `pnpm tauri dev`.

use tauri::AppHandle;
#[cfg(not(target_os = "macos"))]
use tauri_plugin_notification::NotificationExt;

use crate::state::AppState;

/// Fire a best-effort desktop notification. Errors are swallowed (logged at
/// `debug`) because notifications are advisory — the tray + log are the source
/// of truth (F5). No-op paths are handled by the callers below.
///
/// Body/title may carry a tunnel name or SSH error text — NEVER a secret (spec
/// 03 §15). Callers must not pass credentials.
fn show(app: &AppHandle, title: &str, body: &str) {
    #[cfg(target_os = "macos")]
    {
        // macOS: the plugin silently drops in dev/unsigned and we can't detect
        // it (see module docs), so route through osascript, which works
        // regardless of signing. The app handle is unused on this path.
        let _ = app;
        show_via_osascript(title, body);
    }

    #[cfg(not(target_os = "macos"))]
    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => {
            tracing::debug!(title, "notification shown (best-effort)");
        }
        Err(e) => {
            // Expected on headless Linux without a notification daemon.
            // Tray/log remain authoritative.
            tracing::debug!(error = %e, title, "notification not shown (best-effort; tray/log authoritative)");
        }
    }
}

/// macOS notification via `osascript -e 'display notification ...'` (BUG 2).
/// Works unsigned and without a bundle id — unlike the native plugin.
///
/// Runs the child process on the blocking pool so it never blocks the async
/// runtime. `title`/`body` are escaped for the AppleScript string literal
/// (backslash + double-quote) so a name/error containing quotes can neither
/// break the script nor inject AppleScript.
#[cfg(target_os = "macos")]
fn show_via_osascript(title: &str, body: &str) {
    fn escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        escape(body),
        escape(title),
    );
    // spawn_blocking: never block the async runtime on a Command (fire-and-forget).
    tauri::async_runtime::spawn_blocking(move || {
        match std::process::Command::new("osascript")
            .arg("-e")
            .arg(&script)
            .output()
        {
            Ok(out) if out.status.success() => {
                tracing::debug!("notification shown via osascript (best-effort)");
            }
            Ok(out) => {
                tracing::debug!(
                    stderr = %String::from_utf8_lossy(&out.stderr),
                    "osascript notification returned non-zero (tray/log authoritative)"
                );
            }
            Err(e) => {
                tracing::debug!(error = %e, "osascript spawn failed (tray/log authoritative)");
            }
        }
    });
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
