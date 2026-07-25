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

/// Show + focus the main window, apply dock visibility per `showInDock`, and
/// notify the frontend so it rehydrates (AGENTS §5 "app_hydrate on show/boot").
///
/// This is the single show path for every trigger (tray "Open", the
/// `show_window` IPC command, single-instance re-launch), so emitting
/// `WINDOW_FOCUS` here guarantees the frontend re-hydrates on every show — the
/// webview may have been torn down while hidden.
pub fn show_window(app: &AppHandle) {
    // macOS foreground fix (runtime F4): the app runs as an `.accessory` agent
    // (no dock icon), and an accessory app cannot reliably steal focus from the
    // frontmost app — `window.set_focus()` alone shows the window but leaves the
    // previous app (e.g. iTerm) on top. Flip the activation policy to `Regular`
    // BEFORE showing so the process can become active and the window actually
    // comes to the front. The FINAL dock policy is reconciled by
    // `dock::refresh(app, true)` below (back to `Accessory` when
    // `showInDock == false`); the window stays frontmost across that switch.
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
            tracing::error!(error = %e, "failed to set Regular activation policy before show");
        }
    }

    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        tracing::warn!("main window not found; cannot show");
    }
    // Window is now shown → dock visible iff the setting says so. On macOS this
    // may switch the activation policy back to `Accessory`; the just-activated
    // window remains frontmost.
    crate::platform::dock::refresh(app, true);
    // Tell the frontend to rehydrate now that the window is visible again.
    let _ = app.emit(events::WINDOW_FOCUS, ());
}

/// Hide the main window and always drop the dock/taskbar entry (spec 03 §14).
pub fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    // Window hidden → never in the dock, regardless of `showInDock`.
    crate::platform::dock::refresh(app, false);
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
