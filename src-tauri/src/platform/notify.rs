//! Desktop notifications (spec 03 §15).
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
//! **Icon + click-to-open, per platform.**
//!
//! macOS gets a richer path than the cross-platform plugin can offer, because we
//! want two things the plugin's desktop path cannot do:
//! 1. show the **Tunnel Pilot app icon** (not a generic script/starburst icon), and
//! 2. **open the window when the banner or its "Show" button is clicked.**
//!
//! `tauri-plugin-notification`'s desktop implementation (`desktop.rs::show`) is
//! fire-and-forget: it hands a `notify_rust::Notification` to a detached task and
//! discards any handle — there is no action button and no click callback on
//! desktop (`register_action_types` / action events are **mobile-only**). The old
//! macOS path here used `osascript display notification`, which likewise can show
//! neither a real app icon nor a working, callback-wired action button (its icon
//! is Script Editor's — the "starburst").
//!
//! So on macOS we call **`mac-notification-sys` directly** — the *same* native
//! `NSUserNotificationCenter` backend `notify_rust` wraps, minus the fire-and-
//! forget wrapper. It gives us a `MainButton::SingleAction("Show")` button and a
//! **blocking** `send()` that returns a [`NotificationResponse`]; a `Click` (body
//! tap) or `ActionButton` (the "Show" button) then calls
//! [`crate::window::show_window`], which already fronts the window for a tray/
//! accessory app (BUG 1). The blocking `send()` runs on `spawn_blocking` so it
//! NEVER blocks the async runtime.
//!
//! **Icon — dev vs bundled.** On macOS the banner's icon is the *sending app's*
//! icon, chosen by `set_application(bundle_id)`:
//! - **Bundled `.app`:** we send as our own identifier, so the banner shows the
//!   real Tunnel Pilot icon automatically (from `icons/icon.icns`, already set in
//!   `tauri.conf.json`) — no explicit icon needed, and no risk of a wrong one.
//! - **`tauri dev` (unbundled):** the bare binary has no registered bundle id, so
//!   `NSUserNotificationCenter` can't deliver *as us*. We borrow `com.apple.
//!   Terminal` (the same trick the plugin uses — `desktop.rs::show`) purely so the
//!   banner delivers; its corner icon is then Terminal's. As a best-effort we ALSO
//!   pass our resolved icon via `app_icon(...)` when the file is present, so the
//!   logo shows in dev too — but only if it genuinely resolves (never a
//!   placeholder). The authoritative, correct icon is the one in the built `.app`.
//!
//! Windows/Linux keep `tauri-plugin-notification` (the correct native path there;
//! Linux needs a notification daemon, a missing one just logs). Click-to-open is a
//! macOS concern here; the plugin has no desktop click callback to wire on Win/Linux.
//!
//! **osascript fallback.** Kept ONLY for when the native `send()` genuinely fails
//! (e.g. `set_application` can't register the bundle id, or the notification center
//! errors). It shows a plain banner with no icon/button — degraded but better than
//! nothing. Notifications are advisory regardless: the tray icon state + the in-
//! window log/status are the AUTHORITATIVE signal (F5). We never panic, never
//! propagate — a failed notification is logged at `debug` and ignored.
//!
//! **macOS permission timing.** We never `show()` at startup — only on a real
//! connect/error/update event — so any `UNUserNotificationCenter` permission
//! interaction happens at a deliberate, user-triggered moment, not a boot race.

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
    show_native_macos(app, title, body);

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

/// macOS notification via `mac-notification-sys` (the native backend): app icon +
/// a "Show" action button + click/action detection → open the window.
///
/// The blocking `send()` waits for the banner's lifetime (interaction or auto-
/// dismiss), so it runs on the blocking pool and NEVER blocks the async runtime.
/// On failure it degrades to [`show_via_osascript`] (no icon/button).
#[cfg(target_os = "macos")]
fn show_native_macos(app: &AppHandle, title: &str, body: &str) {
    use mac_notification_sys::{MainButton, Notification, NotificationResponse};

    // Deliver as our own identifier in a bundled `.app` (→ real app icon), or as
    // Terminal in `tauri dev` (unbundled → no registered bundle id). `set_application`
    // is a one-shot global `Once`; an `AlreadySet`/`CouldNotSet` result is harmless
    // here — `send()` still falls back to a sane default and we catch hard errors.
    let bundle_id = if tauri::is_dev() {
        "com.apple.Terminal".to_string()
    } else {
        app.config().identifier.clone()
    };
    let _ = mac_notification_sys::set_application(&bundle_id);

    // In dev the corner icon is Terminal's; best-effort surface our real logo via
    // `app_icon` if it resolves on disk. In a bundle the sending-app icon is
    // already correct, so we don't double it up.
    let icon_path = if tauri::is_dev() {
        resolve_icon_path(app)
    } else {
        None
    };

    let app = app.clone();
    let title = title.to_string();
    let body = body.to_string();
    // spawn_blocking: `send()` blocks until the banner is interacted with or
    // auto-dismisses — must never run on the async runtime.
    tauri::async_runtime::spawn_blocking(move || {
        let mut notification = Notification::new();
        notification
            .title(&title)
            .message(&body)
            .main_button(MainButton::SingleAction("Show"));
        if let Some(ref path) = icon_path {
            notification.app_icon(path);
        }
        match notification.send() {
            // Body tap OR the "Show" button → bring the window to the front.
            Ok(NotificationResponse::Click) | Ok(NotificationResponse::ActionButton(_)) => {
                tracing::debug!("notification clicked; showing window");
                crate::window::show_window(&app);
            }
            // Ignored / closed / auto-dismissed — nothing to do.
            Ok(_) => {
                tracing::debug!("notification shown via mac-notification-sys (best-effort)");
            }
            Err(e) => {
                tracing::debug!(error = %e, "native notification failed; falling back to osascript");
                show_via_osascript(&title, &body);
            }
        }
    });
}

/// Resolve the app icon file inside the macOS bundle (`Contents/Resources`) for
/// the dev `app_icon` best-effort. Returns `None` if nothing resolves, so we
/// never pass a non-existent/placeholder path (task: no wrong icon).
#[cfg(target_os = "macos")]
fn resolve_icon_path(app: &AppHandle) -> Option<String> {
    use tauri::Manager;
    let dir = app.path().resource_dir().ok()?;
    for name in ["icon.icns", "128x128@2x.png", "128x128.png", "32x32.png"] {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

/// macOS notification via `osascript -e 'display notification ...'` — the degraded
/// fallback when the native `send()` fails. Works unsigned and without a bundle
/// id, but shows NO app icon and NO working action button.
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
                tracing::debug!("notification shown via osascript (fallback, best-effort)");
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
