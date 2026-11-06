//! Settings service fns (spec 02 §6.3), Tauri-free. Applying settings is split in
//! two: this module stores + persists + emits (every transport), and the desktop
//! command layers the OS side effects (autostart, dock policy) on top.

use std::sync::Arc;

use crate::error::AppError;
use crate::state::models::AppSettings;
use crate::state::AppState;

/// `get_settings` — the current settings.
pub fn get_settings(state: &Arc<AppState>) -> AppSettings {
    state.settings_snapshot()
}

/// `update_settings` (core) — replace, persist, emit `settings://changed`.
pub async fn update_settings(
    state: &Arc<AppState>,
    input: AppSettings,
) -> Result<AppSettings, AppError> {
    state.set_settings(input.clone());
    state.persist_settings().await?;
    state.emit_settings_changed();
    Ok(input)
}
