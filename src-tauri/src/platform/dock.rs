//! Dock/taskbar visibility.
//!
//! # macOS activation model (policy = f(window visibility, `showInDock`))
//!
//! The macOS activation policy is a function of BOTH whether the window is
//! visible AND the `showInDock` setting:
//!
//! ```text
//! policy = if window_visible || show_in_dock { Regular } else { Accessory }
//! ```
//!
//! Why both, not the setting alone: an `Accessory` (agent / `LSUIElement`) app
//! does NOT own the macOS menu bar and does NOT appear in the Cmd+Tab switcher,
//! *even while showing a window*. So with `showInDock` off (the default) an open
//! window left the previous app's menus (e.g. iTerm2) in the menu bar and the
//! app absent from Cmd+Tab. Making the policy `Regular` whenever a window is
//! visible gives the open window a normal-app feel: own menu bar + Cmd+Tab entry
//! + dock icon.
//!
//! The old `Regular → Accessory`-while-a-window-is-open transition ordered the
//! window out (the "vanish" bug). This model CANNOT trigger it: we only ever
//! transition TO `Accessory` when the window is HIDDEN (see the matrix — the
//! only `Accessory` cell is window-closed). Every window-open state is `Regular`.
//!
//! ## Matrix (macOS)
//!
//! | state                          | policy      | effect                                                    |
//! |--------------------------------|-------------|-----------------------------------------------------------|
//! | window OPEN (any `showInDock`) | `Regular`   | owns menu bar + in Cmd+Tab + dock icon (normal-app feel)  |
//! | window CLOSED + `showInDock` ON  | `Regular` | dock icon persists + in Cmd+Tab (background app w/ dock)  |
//! | window CLOSED + `showInDock` OFF | `Accessory` | tray-only: no dock, not in Cmd+Tab                      |
//!
//! Applied on boot ([`crate::run`]), on window show/hide
//! ([`crate::window::show_window`] / [`crate::window::hide_window`]), and on a
//! runtime `showInDock` toggle ([`crate::commands::settings::update_settings`]),
//! always with the CURRENT window visibility.
//!
//! On Windows/Linux there is no activation policy; the taskbar entry is driven by
//! `showInDock` for the shown state ([`taskbar_visible`]). A hidden window has no
//! taskbar entry regardless of the setting.

use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::state::AppState;

/// Pure decision (macOS): should the app be `Regular` (own the menu bar + appear
/// in Cmd+Tab + have a dock icon)? `Regular` iff the window is visible OR
/// `showInDock` is on; otherwise `Accessory` (tray-only). Used by
/// [`apply_dock_policy`] so the decision is unit-testable without a display.
///
/// The `window_visible` term is what fixes the "accessory app doesn't own the
/// menu bar / isn't in Cmd+Tab while showing a window" bug. It is also why the
/// vanish bug cannot recur: `Accessory` is only ever reached when
/// `!window_visible`, so we never flip a *visible* window to `Accessory`.
pub fn macos_should_be_regular(window_visible: bool, show_in_dock: bool) -> bool {
    window_visible || show_in_dock
}

/// Pure decision (Windows/Linux): is the taskbar entry visible? `true` only when
/// the window is shown AND `showInDock` is on (a hidden window has no taskbar
/// entry regardless). macOS does not use this — its dock is governed by the
/// activation policy ([`macos_should_be_regular`]).
pub fn taskbar_visible(window_shown: bool, show_in_dock: bool) -> bool {
    window_shown && show_in_dock
}

/// Apply the platform dock/taskbar policy from the CURRENT window visibility and
/// the `showInDock` setting.
///
/// - macOS: `set_activation_policy(Regular)` iff [`macos_should_be_regular`],
///   else `Accessory`. Because `Accessory` is only reached when the window is
///   hidden, this never orders an open window out (no vanish bug).
/// - Windows/Linux: `set_skip_taskbar(!(window_visible && show_in_dock))`.
///
/// Call on boot and on every show/hide/settings-change with the real window
/// visibility.
pub fn apply_dock_policy(app: &AppHandle, window_visible: bool, show_in_dock: bool) {
    #[cfg(target_os = "macos")]
    {
        let policy = if macos_should_be_regular(window_visible, show_in_dock) {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        if let Err(e) = app.set_activation_policy(policy) {
            tracing::error!(
                error = %e,
                window_visible,
                show_in_dock,
                "failed to set macOS activation policy"
            );
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let visible = taskbar_visible(window_visible, show_in_dock);
        if let Some(window) = app.get_webview_window(crate::window::MAIN_WINDOW) {
            // skipTaskbar is the inverse of "visible in taskbar".
            if let Err(e) = window.set_skip_taskbar(!visible) {
                tracing::error!(error = %e, visible, "failed to set skipTaskbar");
            }
        }
    }
}

/// Apply the dock/taskbar policy reading `showInDock` from [`AppState`] and the
/// live window visibility from the main window (`is_visible()`), defaulting to
/// hidden if either cannot be resolved. Convenience wrapper over
/// [`apply_dock_policy`] for callers that only have the [`AppHandle`].
pub fn refresh_dock_policy(app: &AppHandle) {
    let show_in_dock = app
        .try_state::<Arc<AppState>>()
        .map(|s| s.settings_snapshot().show_in_dock)
        .unwrap_or(false);
    let window_visible = app
        .get_webview_window(crate::window::MAIN_WINDOW)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    apply_dock_policy(app, window_visible, show_in_dock);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The macOS policy is a function of BOTH window visibility and the setting.
    /// Crucially the window-open cases are ALL `Regular` (own menu bar, Cmd+Tab,
    /// dock icon) regardless of `showInDock` — that is the fix — and the only
    /// `Accessory` case is window-closed with `showInDock` off (tray-only).
    /// Because `Accessory` is never reached while the window is visible, the old
    /// visible-window vanish transition cannot happen.
    #[test]
    fn macos_regular_when_window_visible_or_show_in_dock() {
        // window OPEN → Regular regardless of the setting (THE FIX).
        assert!(macos_should_be_regular(true, true));
        assert!(macos_should_be_regular(true, false));
        // window CLOSED + showInDock ON → Regular (dock persists + Cmd+Tab).
        assert!(macos_should_be_regular(false, true));
        // window CLOSED + showInDock OFF → Accessory (tray-only).
        assert!(!macos_should_be_regular(false, false));
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
