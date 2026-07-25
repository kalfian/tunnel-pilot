//! Window lifecycle: hide-on-close intercept, `show_window`/`hide_window`,
//! single-instance re-show, and graceful quit (spec 03 §§11,14).
//!
//! The app lives in the tray: closing the window HIDES it (the process keeps
//! running); only `quit_app` actually exits, after tearing down every tunnel.
//!
//! Activation model (runtime BUG A fix): the dock/taskbar policy follows WINDOW
//! VISIBILITY, not the `showInDock` setting. Window shown ⇒ macOS `Regular`
//! (dock icon + frontmost window); window hidden ⇒ macOS `Accessory` (tray-only,
//! no dock). On macOS a `Regular → Accessory` flip while a window is open orders
//! it out, so we never switch to `Accessory` while the window is visible — that
//! is exactly what made the window vanish before. `showInDock` still drives the
//! Windows/Linux taskbar entry. Applied here via `platform::dock`.

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
    tracing::info!("showing main window (activation → Regular)");
    // macOS foreground fix (runtime F4 + BUG A): the app sits in the tray as an
    // `.accessory` agent (no dock icon) while hidden, and an accessory app cannot
    // reliably steal focus — `window.set_focus()` alone shows the window but
    // leaves the previously-frontmost app (e.g. iTerm) on top. Flip to `Regular`
    // BEFORE showing so the process becomes active and the window comes frontmost.
    // Crucially we STAY `Regular` while shown: `dock::refresh(app, true)` below
    // now keeps `Regular` (it no longer flips back to `Accessory` for
    // `showInDock == false`, which used to order the just-shown window out).
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
    // Window is shown → macOS stays `Regular` (dock icon present); Win/Linux
    // taskbar follows `showInDock`.
    crate::platform::dock::refresh(app, true);
    // Tell the frontend to rehydrate now that the window is visible again.
    let _ = app.emit(events::WINDOW_FOCUS, ());
}

/// Hide the main window and drop the dock/taskbar entry (spec 03 §14): macOS
/// returns to `Accessory` (tray-only), Win/Linux hide the taskbar entry.
pub fn hide_window(app: &AppHandle) {
    tracing::info!("hiding main window to tray (activation → Accessory)");
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    // Window hidden → macOS `Accessory` (no dock icon), Win/Linux taskbar dropped.
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
