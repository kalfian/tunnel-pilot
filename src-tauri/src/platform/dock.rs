//! Dock/taskbar visibility.
//!
//! # macOS activation model (BUG 1 fix — policy follows the SETTING)
//!
//! The macOS activation policy follows the `showInDock` **setting alone**, NOT
//! window visibility:
//!
//! - `showInDock == true`  ⇒ [`tauri::ActivationPolicy::Regular`]  (dock icon)
//! - `showInDock == false` ⇒ [`tauri::ActivationPolicy::Accessory`] (tray-only)
//!
//! It is set once on boot from the loaded setting, and again whenever the
//! setting changes (the settings-update path). It is **deliberately never
//! touched on window show/hide**, because a `Regular → Accessory` transition
//! *while a window is open* orders the window out (macOS hides it). The earlier
//! fix tied the policy to visibility, so closing the window flipped it to
//! `Accessory` and the dock icon vanished even with `showInDock` on — that is
//! the bug. With the policy pinned to the setting, the dock icon now persists
//! across window open/close.
//!
//! Fronting the window when the app is `Accessory` (showInDock off) is handled
//! by [`crate::window::show_window`] via `activateIgnoringOtherApps` — an
//! *activation*, not a *policy transition*, so it fronts the app+window without
//! adding a dock icon and without triggering the vanish.
//!
//! ## Matrix (macOS)
//!
//! | `showInDock` | window closed          | window open                     |
//! |--------------|------------------------|---------------------------------|
//! | ON           | dock icon present      | dock icon present + window      |
//! | OFF          | no dock icon           | no dock icon, window still fronts |
//!
//! Toggling `showInDock` at runtime re-applies the policy immediately (the
//! settings command calls [`apply_dock_policy`]).
//!
//! On Windows/Linux there is no activation policy; the taskbar entry is driven
//! by `showInDock` for the shown state ([`taskbar_visible`] / [`apply_taskbar`]).
//! A hidden window has no taskbar entry regardless of the setting.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Pure decision (macOS): should the dock icon be present? Follows the
/// `showInDock` setting **alone** — window visibility is intentionally NOT a
/// parameter (BUG 1: the dock icon persists whether the window is open or
/// closed). Used by [`apply_dock_policy`] so the decision is unit-testable
/// without a display.
pub fn macos_dock_icon_present(show_in_dock: bool) -> bool {
    show_in_dock
}

/// Pure decision (Windows/Linux): is the taskbar entry visible? `true` only
/// when the window is shown AND `showInDock` is on (a hidden window has no
/// taskbar entry regardless). macOS does not use this — its dock is governed by
/// the activation policy ([`macos_dock_icon_present`]).
pub fn taskbar_visible(window_shown: bool, show_in_dock: bool) -> bool {
    window_shown && show_in_dock
}

/// Apply the macOS dock/activation policy from the `showInDock` **setting alone**
/// (BUG 1). `Regular` ⇒ dock icon, `Accessory` ⇒ tray-only. Call on boot and on
/// every settings change — NEVER on window show/hide (see module docs: tying the
/// policy to visibility makes the dock icon vanish on close and orders an open
/// window out). No-op on Windows/Linux (no activation policy there).
pub fn apply_dock_policy(app: &AppHandle, show_in_dock: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if macos_dock_icon_present(show_in_dock) {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(e) = app.set_activation_policy(policy) {
            tracing::error!(error = %e, show_in_dock, "failed to set macOS activation policy");
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, show_in_dock);
}

/// Apply the macOS dock policy reading `showInDock` from [`AppState`]. No-op on
/// Windows/Linux.
pub fn refresh_dock_policy(app: &AppHandle) {
    let show_in_dock = app
        .try_state::<Arc<AppState>>()
        .map(|s| s.settings_snapshot().show_in_dock)
        .unwrap_or(false);
    apply_dock_policy(app, show_in_dock);
}

/// Apply the Windows/Linux taskbar entry for the given shown state, driven by
/// `showInDock`. No-op on macOS (the dock is governed by the activation policy).
pub fn apply_taskbar(app: &AppHandle, window_shown: bool, show_in_dock: bool) {
    #[cfg(not(target_os = "macos"))]
    {
        let visible = taskbar_visible(window_shown, show_in_dock);
        if let Some(window) = app.get_webview_window(crate::window::MAIN_WINDOW) {
            // skipTaskbar is the inverse of "visible in taskbar".
            if let Err(e) = window.set_skip_taskbar(!visible) {
                tracing::error!(error = %e, visible, "failed to set skipTaskbar");
            }
        }
    }
    #[cfg(target_os = "macos")]
    let _ = (app, window_shown, show_in_dock);
}

/// Apply the Windows/Linux taskbar for the given shown state, reading
/// `showInDock` from [`AppState`]. No-op on macOS.
pub fn refresh_taskbar(app: &AppHandle, window_shown: bool) {
    let show_in_dock = app
        .try_state::<Arc<AppState>>()
        .map(|s| s.settings_snapshot().show_in_dock)
        .unwrap_or(false);
    apply_taskbar(app, window_shown, show_in_dock);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// BUG 1: the macOS dock decision follows the SETTING, not window
    /// visibility. There is no window-shown parameter, so open vs closed cannot
    /// change dock presence — the icon persists across window open/close.
    #[test]
    fn macos_dock_follows_setting_not_visibility() {
        assert!(macos_dock_icon_present(true));
        assert!(!macos_dock_icon_present(false));
    }

    /// Windows/Linux taskbar: visible only when shown AND enabled; a hidden
    /// window is never in the taskbar regardless of the setting.
    #[test]
    fn taskbar_visible_only_when_shown_and_enabled() {
        assert!(taskbar_visible(true, true));
        assert!(!taskbar_visible(false, true));
        assert!(!taskbar_visible(true, false));
        assert!(!taskbar_visible(false, false));
    }
}
