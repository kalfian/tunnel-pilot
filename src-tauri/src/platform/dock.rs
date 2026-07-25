//! Dock/taskbar visibility: macOS `AppHandle::set_activation_policy`
//! (Regular/Accessory — NOT objc FFI, F11); Win/Linux `set_skip_taskbar`.
//!
//! # macOS activation model (runtime BUG A fix)
//!
//! On macOS the activation policy MUST follow **window visibility**, not the
//! `showInDock` setting:
//!
//! - **Window shown ⇒ `ActivationPolicy::Regular`** (dock icon present, window
//!   visible + frontmost).
//! - **Window hidden ⇒ `ActivationPolicy::Accessory`** (tray-only, no dock icon).
//!
//! Reason: transitioning `Regular → Accessory` *while a window is open* orders
//! the window out on macOS — it silently vanishes. The old model flipped back to
//! `Accessory` right after showing when `showInDock == false`, which is exactly
//! why the window never appeared. So on macOS `showInDock` can no longer mean
//! "no dock icon while the window is shown" — a visible macOS window always
//! carries a dock icon (a working visible window is preferred over a hidden
//! one). `showInDock` still governs the Windows/Linux taskbar entry, where no
//! vanish problem exists.
//!
//! The taskbar decision (non-macOS) stays a pure function ([`dock_visible`]) so
//! the truth table (window-shown × showInDock) is unit-testable without a
//! display: visible only when the window is shown AND `showInDock` is on.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Pure decision: is the **taskbar** entry visible (Windows/Linux)? (spec 03 §13)
///
/// `true` only when the window is shown and `showInDock` is enabled; a hidden
/// window is never in the taskbar regardless of the setting.
///
/// Note: this governs the Win/Linux taskbar only. On macOS the dock icon follows
/// window visibility alone (see the module docs) — `showInDock` is not consulted
/// for the shown state there.
pub fn dock_visible(window_shown: bool, show_in_dock: bool) -> bool {
    window_shown && show_in_dock
}

/// Apply dock/taskbar visibility for the given window-shown state.
///
/// - macOS: activation policy follows `window_shown` alone — `Regular` when the
///   window is shown, `Accessory` when hidden (the Tauri v2 built-in API, NOT
///   objc FFI, F11). `show_in_dock` is intentionally ignored here (see module
///   docs: flipping to `Accessory` while shown hides the window).
/// - Windows/Linux: toggle `skipTaskbar` per [`dock_visible`] (`showInDock`).
pub fn apply(app: &AppHandle, window_shown: bool, show_in_dock: bool) {
    #[cfg(target_os = "macos")]
    {
        // macOS ignores the setting for the shown state — the policy tracks
        // window visibility so a shown window is never ordered out.
        let _ = show_in_dock;
        let policy = if window_shown {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(e) = app.set_activation_policy(policy) {
            tracing::error!(error = %e, window_shown, "failed to set macOS activation policy");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let visible = dock_visible(window_shown, show_in_dock);
        if let Some(window) = app.get_webview_window(crate::window::MAIN_WINDOW) {
            // skipTaskbar is the inverse of "visible in taskbar".
            if let Err(e) = window.set_skip_taskbar(!visible) {
                tracing::error!(error = %e, visible, "failed to set skipTaskbar");
            }
        }
    }
}

/// Recompute + apply dock/taskbar visibility for a given window-shown state,
/// reading the current `showInDock` setting from `AppState`. Called by
/// `window::show_window` / `hide_window` and by the settings command when
/// `showInDock` changes. On macOS the policy follows `window_shown` alone.
pub fn refresh(app: &AppHandle, window_shown: bool) {
    let show_in_dock = app
        .try_state::<Arc<AppState>>()
        .map(|s| s.settings_snapshot().show_in_dock)
        .unwrap_or(false);
    apply(app, window_shown, show_in_dock);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_only_when_shown_and_enabled() {
        assert!(dock_visible(true, true));
    }

    #[test]
    fn hidden_window_is_always_dockless() {
        assert!(!dock_visible(false, true));
        assert!(!dock_visible(false, false));
    }

    #[test]
    fn shown_but_setting_off_is_dockless() {
        assert!(!dock_visible(true, false));
    }
}
