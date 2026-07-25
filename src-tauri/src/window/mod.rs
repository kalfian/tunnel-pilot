//! Window lifecycle: hide-on-close intercept, `show_window`/`hide_window`,
//! single-instance re-show, and graceful quit (spec 03 §§11,14).
//!
//! The app lives in the tray: closing the window HIDES it (the process keeps
//! running); only `quit_app` actually exits, after tearing down every tunnel.
//!
//! Activation model: the macOS dock/activation policy is a function of BOTH
//! window visibility AND the `showInDock` setting —
//! `Regular` iff `window_visible || show_in_dock`, else `Accessory` (see
//! [`crate::platform::dock`]). It is applied on boot, on settings-change, and
//! HERE on show/hide with the current visibility.
//!
//! - `show_window` sets `Regular` (the window is now visible) BEFORE showing, so
//!   the app owns the menu bar + appears in Cmd+Tab + has a dock icon — a
//!   normal-app feel. `activateIgnoringOtherApps` then fronts it as the active
//!   app. We never flip to `Accessory` here.
//! - `hide_window` recomputes for the hidden state: `Regular` if `showInDock`
//!   (dock persists), else `Accessory` (tray-only).
//!
//! The old vanish bug (`Regular → Accessory` while a window is open ordered it
//! out) cannot recur: we only transition to `Accessory` when the window is
//! HIDDEN. `showInDock` still drives the Windows/Linux taskbar entry.

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
    // Policy FIRST (window is about to be visible): `Regular` so the app owns
    // the macOS menu bar, appears in Cmd+Tab, and gets a dock icon — the
    // normal-app feel. `window_visible = true` makes `apply_dock_policy` compute
    // `Regular` regardless of `showInDock` (see `platform::dock` matrix). This
    // is a transition TO `Regular` (never TO `Accessory` while visible), so the
    // vanish bug cannot occur. On Windows/Linux this reflects the taskbar entry.
    let show_in_dock = state_show_in_dock(app);
    crate::platform::dock::apply_dock_policy(app, true, show_in_dock);
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        tracing::warn!("main window not found; cannot show");
    }
    // macOS: make Tunnel Pilot the frontmost app so it actually OWNS the menu bar
    // now (a `Regular` app that is merely shown may still sit behind the previous
    // frontmost app, e.g. iTerm, leaving its menus up). `activateIgnoringOtherApps`
    // fronts the app+window as an ACTIVATION (not a policy transition). AppKit
    // must run on the main thread.
    #[cfg(target_os = "macos")]
    {
        let _ = app.run_on_main_thread(|| {
            crate::platform::macos::activate_ignoring_other_apps();
        });
    }
    // Tell the frontend to rehydrate now that the window is visible again.
    let _ = app.emit(events::WINDOW_FOCUS, ());
}

/// Read `showInDock` from [`AppState`], defaulting to `false` if unavailable.
fn state_show_in_dock(app: &AppHandle) -> bool {
    app.try_state::<Arc<AppState>>()
        .map(|s| s.settings_snapshot().show_in_dock)
        .unwrap_or(false)
}

/// Hide the main window to the tray (spec 03 §14).
///
/// After hiding, recompute the macOS activation policy for the window-HIDDEN
/// state: `Regular` if `showInDock` is on (dock icon persists, still in Cmd+Tab)
/// else `Accessory` (tray-only, no dock, not in Cmd+Tab). This is the ONLY place
/// we may transition to `Accessory`, and it runs only once the window is already
/// hidden — so the old visible-window vanish bug cannot occur. On Windows/Linux
/// this drops the taskbar entry.
pub fn hide_window(app: &AppHandle) {
    tracing::info!("hiding main window to tray");
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.hide();
    }
    let show_in_dock = state_show_in_dock(app);
    crate::platform::dock::apply_dock_policy(app, false, show_in_dock);
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
