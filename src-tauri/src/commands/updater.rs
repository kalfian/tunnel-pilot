//! Updater commands (spec 02 §6.6): `check_update`, `install_update`,
//! `skip_update`.
//!
//! The real self-update flow (minisign-signed bundle download + verify +
//! install, `update://progress`) is wired in M6 (spec 03 §16); `check_update`
//! and `install_update` are deferred stubs here so the command surface + ACL are
//! complete. `skip_update` is fully functional now — it just records
//! `lastSkippedVersion` in settings.

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::state::models::UpdateStatus;
use crate::state::AppState;

/// `check_update` — availability snapshot. M6 wires the real `tauri-plugin-updater`
/// check; until then no update is ever reported available.
#[tauri::command]
pub async fn check_update() -> Result<UpdateStatus, AppError> {
    Ok(UpdateStatus::default())
}

/// `install_update` — download + verify + install the pending update. Deferred
/// to M6 (signed-bundle updater); returns a clear error until then rather than
/// silently no-op'ing.
#[tauri::command]
pub async fn install_update() -> Result<(), AppError> {
    Err(AppError::Updater(
        "self-update is not available yet (wired in M6)".into(),
    ))
}

/// `skip_update` — remember that the user dismissed `version` so it is not
/// offered again (spec 02 §6.6). Persists `lastSkippedVersion` and notifies the
/// frontend.
#[tauri::command]
pub async fn skip_update(state: State<'_, Arc<AppState>>, version: String) -> Result<(), AppError> {
    let state = state.inner();
    let mut settings = state.settings_snapshot();
    settings.last_skipped_version = Some(version);
    state.set_settings(settings);
    state.persist_settings().await?;
    state.emit_settings_changed();
    Ok(())
}
