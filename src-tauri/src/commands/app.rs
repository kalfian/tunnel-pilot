//! App/window commands (spec 02 §6.7): `app_hydrate` (one-shot snapshot),
//! `show_window`, `hide_window`, `quit_app`.
//!
//! `app_hydrate` is the rehydrate contract (AGENTS §5): a single call returns
//! everything the frontend needs on window show/boot — forwards, groups,
//! settings, logs, live runtimes, keychain availability, and update status — so
//! the webview can be torn down while hidden and fully rebuilt on show.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::error::AppError;
use crate::state::models::{AppSnapshot, UpdateStatus};
use crate::state::AppState;

/// `app_hydrate` — the full boot/rehydrate snapshot (spec 04 §8). The `update`
/// field is a not-available default until the updater is wired in M6.
#[tauri::command]
pub fn app_hydrate(state: State<'_, Arc<AppState>>) -> AppSnapshot {
    state.app_snapshot(UpdateStatus::default())
}

/// `show_window` — show + focus the main window (tray "Open" / single-instance).
#[tauri::command]
pub fn show_window(app: AppHandle) {
    crate::window::show_window(&app);
}

/// `hide_window` — hide the window (custom close button); the app stays in tray.
#[tauri::command]
pub fn hide_window(app: AppHandle) {
    crate::window::hide_window(&app);
}

/// `quit_app` — tear down every live tunnel, then exit (tray "Quit" / palette).
#[tauri::command]
pub fn quit_app(app: AppHandle) -> Result<(), AppError> {
    crate::window::quit_app(&app);
    Ok(())
}
