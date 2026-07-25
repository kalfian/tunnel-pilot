//! Settings commands (spec 02 §6.3): `get_settings`, `update_settings`.
//!
//! `update_settings` persists the new settings then applies the platform side
//! effects that must react immediately (spec 02 §6.3): launch-at-login
//! reconcile, dock/taskbar visibility, and a `settings://changed` emit so the
//! frontend re-applies the theme. Theme itself is a frontend concern (CSS driven
//! by `themeMode`); the backend only persists + notifies.

use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::error::AppError;
use crate::state::models::AppSettings;
use crate::state::AppState;

/// `get_settings` — current settings (boot / rehydrate).
#[tauri::command]
pub fn get_settings(state: State<'_, Arc<AppState>>) -> AppSettings {
    state.settings_snapshot()
}

/// `update_settings` — persist the new settings and apply side effects
/// (autostart reconcile + dock visibility), then emit `settings://changed`.
#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    input: AppSettings,
) -> Result<AppSettings, AppError> {
    let state = state.inner();
    state.set_settings(input.clone());
    state.persist_settings().await?;

    // Launch-at-login: drive the OS registration to match (idempotent).
    crate::platform::autostart::reconcile(&app, input.launch_at_login);

    // Dock/taskbar (BUG 1): the macOS activation policy follows the `showInDock`
    // SETTING and is re-applied immediately here so a runtime toggle takes effect
    // at once — independent of whether the window is currently shown. The
    // Windows/Linux taskbar entry follows `showInDock` for the current shown
    // state.
    crate::platform::dock::apply_dock_policy(&app, input.show_in_dock);
    let window_shown = app
        .get_webview_window(crate::window::MAIN_WINDOW)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    crate::platform::dock::apply_taskbar(&app, window_shown, input.show_in_dock);

    // Theme is applied on the frontend; notify it (and any other listener) that
    // settings changed so it can re-render.
    state.emit_settings_changed();

    Ok(input)
}
