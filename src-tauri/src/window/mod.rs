//! Window lifecycle: hide-on-close intercept, `show_window`/`hide_window`,
//! single-instance re-show, and graceful quit (spec 03 §§11,14).
//!
//! The app lives in the tray: closing the window HIDES it (the process keeps
//! running); only `quit_app` actually exits, after tearing down every tunnel.
//!
//! Activation model (BUG 1 fix): the macOS dock/activation policy follows the
//! `showInDock` SETTING alone, NOT window visibility — it is applied on boot and
//! on settings-change (see [`crate::platform::dock`]) and is NEVER touched on
//! show/hide here. A `Regular → Accessory` flip while a window is open orders it
//! out (the vanish bug), so `show_window`/`hide_window` leave the policy alone:
//! the dock icon persists across open/close iff `showInDock` is on. Fronting an
//! `Accessory` app (showInDock off) when showing is done via
//! `activateIgnoringOtherApps` — an activation, not a policy transition, so it
//! fronts the window with no dock icon and no vanish. `showInDock` still drives
//! the Windows/Linux taskbar entry for the shown state.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::events;
use crate::state::AppState;

/// The main window label (matches `tauri.conf.json`).
pub const MAIN_WINDOW: &str = "main";

/// Show + focus the main window, apply dock visibility per `showInDock`, and
/// notify the frontend so it rehydrates (AGENTS §5 "app_hydrate on show/boot").
///
/// This is the single show path for every trigger (tray "Settings", the
/// `show_window` IPC command, single-instance re-launch), so emitting
/// `WINDOW_FOCUS` here guarantees the frontend re-hydrates on every show — the
/// webview may have been torn down while hidden.
pub fn show_window(app: &AppHandle) {
    tracing::info!("showing main window");
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        tracing::warn!("main window not found; cannot show");
    }
    // macOS foreground fix (BUG 1): the app may sit in the tray as an
    // `.accessory` agent (showInDock off, no dock icon), where `set_focus()`
    // alone shows the window but leaves the previously-frontmost app (e.g.
    // iTerm) on top. We front it via `activateIgnoringOtherApps` — an
    // ACTIVATION, not a POLICY TRANSITION — so it comes frontmost WITHOUT adding
    // a dock icon and WITHOUT the `Regular → Accessory` vanish. We do NOT touch
    // the activation policy here: it follows the `showInDock` SETTING (applied on
    // boot/settings-change), so the dock icon persists across open/close. AppKit
    // must run on the main thread.
    #[cfg(target_os = "macos")]
    {
        let _ = app.run_on_main_thread(|| {
            crate::platform::macos::activate_ignoring_other_apps();
        });
    }
    // Win/Linux: reflect the taskbar entry for the shown state per `showInDock`.
    // macOS dock is NOT touched here (governed by the activation policy).
    crate::platform::dock::refresh_taskbar(app, true);
    // Tell the frontend to rehydrate now that the window is visible again.
    let _ = app.emit(events::WINDOW_FOCUS, ());
}

/// Hide the main window to the tray (spec 03 §14).
///
/// BUG 1: `window.hide()` ONLY — the macOS activation policy is deliberately
/// left untouched so the dock icon persists while hidden iff `showInDock` is on
/// (touching it here is exactly what removed the icon on close before). On
/// Windows/Linux a hidden window has no taskbar entry regardless, so there is
/// nothing to drop.
pub fn hide_window(app: &AppHandle) {
    tracing::info!("hiding main window to tray");
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
}

/// Single-instance re-show: a second launch focuses the existing window (spec
/// 03 §11). `show_window` already emits `WINDOW_FOCUS`, so the frontend refresh
/// is covered without a second emit here.
pub fn focus_from_second_instance(app: &AppHandle) {
    show_window(app);
}

/// Install the hide-on-close intercept on the main window (spec 03 §14): the OS
/// close button hides the window and keeps the app alive in the tray.
pub fn install_close_handler(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else {
        tracing::warn!("main window not found; hide-on-close not installed");
        return;
    };
    let handle = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            // Intercept: hide instead of closing so the tray app persists.
            api.prevent_close();
            hide_window(&handle);
        }
    });
}

/// Quit the app for real (tray "Quit" / palette): tear down every live tunnel
/// so no port stays bound, then exit (spec 03 §14 acceptance). The teardown runs
/// on the async runtime; the process exits once it completes.
pub fn quit_app(app: &AppHandle) {
    let handle = app.clone();
    let state = match app.try_state::<Arc<AppState>>() {
        Some(s) => s.inner().clone(),
        None => {
            // No state managed — nothing to clean up; exit immediately.
            app.exit(0);
            return;
        }
    };
    tauri::async_runtime::spawn(async move {
        for id in state.registry.all_ids() {
            // Treat quit as user-initiated (silent); ignore per-tunnel errors —
            // we exit regardless.
            let _ = crate::ssh::engine::disconnect_forward(&state, &id, true).await;
        }
        tracing::info!("all tunnels torn down; exiting");
        handle.exit(0);
    });
}
