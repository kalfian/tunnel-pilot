//! Window lifecycle: hide-on-close intercept, `show_window`/`hide_window`,
//! single-instance re-show, and graceful quit (spec 03 §§11,14).
//!
//! The app lives in the tray: closing the window HIDES it (the process keeps
//! running); only `quit_app` actually exits, after tearing down every tunnel.
//! Dock/taskbar visibility is applied here via `platform::dock` — show follows
//! the `showInDock` setting, hide is always dock-less.

use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::events;
use crate::state::AppState;

/// The main window label (matches `tauri.conf.json`).
pub const MAIN_WINDOW: &str = "main";

/// Show + focus the main window and apply dock visibility per `showInDock`.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        tracing::warn!("main window not found; cannot show");
    }
    // Window is now shown → dock visible iff the setting says so.
    crate::platform::dock::refresh(app, true);
}

/// Hide the main window and always drop the dock/taskbar entry (spec 03 §14).
pub fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    // Window hidden → never in the dock, regardless of `showInDock`.
    crate::platform::dock::refresh(app, false);
}

/// Single-instance re-show: a second launch focuses the existing window and
/// notifies the frontend so it can refresh (spec 03 §11).
pub fn focus_from_second_instance(app: &AppHandle) {
    show_window(app);
    let _ = app.emit(events::WINDOW_FOCUS, ());
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
