//! Dock/taskbar visibility: macOS `AppHandle::set_activation_policy`
//! (Regular/Accessory — NOT objc FFI, F11); Win/Linux `set_skip_taskbar`.
//! Driven by `showInDock` (spec 03 §13).
//!
//! The visibility decision is a pure function ([`dock_visible`]) so the truth
//! table (window-shown × showInDock) is unit-testable without a display:
//! visible only when the window is shown AND `showInDock` is on; a hidden window
//! is always dock-less.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Pure decision: is the dock/taskbar entry visible? (spec 03 §13)
///
/// `true` only when the window is shown and `showInDock` is enabled; a hidden
/// window is never in the dock regardless of the setting.
pub fn dock_visible(window_shown: bool, show_in_dock: bool) -> bool {
    window_shown && show_in_dock
}

/// Apply dock/taskbar visibility. macOS switches the activation policy via the
/// Tauri v2 built-in API (NOT objc FFI, F11); Windows/Linux toggle skipTaskbar.
pub fn apply(app: &AppHandle, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if visible {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(e) = app.set_activation_policy(policy) {
            tracing::error!(error = %e, visible, "failed to set macOS activation policy");
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(window) = app.get_webview_window(crate::window::MAIN_WINDOW) {
            // skipTaskbar is the inverse of "visible in taskbar".
            if let Err(e) = window.set_skip_taskbar(!visible) {
                tracing::error!(error = %e, visible, "failed to set skipTaskbar");
            }
        }
    }
}

/// Recompute + apply dock visibility for a given window-shown state, reading the
/// current `showInDock` setting from `AppState`. Called by `window::show_window`
/// / `hide_window` and (M4) by the settings command when `showInDock` changes.
pub fn refresh(app: &AppHandle, window_shown: bool) {
    let show_in_dock = app
        .try_state::<Arc<AppState>>()
        .map(|s| s.settings_snapshot().show_in_dock)
        .unwrap_or(false);
    apply(app, dock_visible(window_shown, show_in_dock));
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
