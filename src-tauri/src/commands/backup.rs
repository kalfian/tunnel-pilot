//! Backup commands (spec 02 §6.5): thin wrappers over [`crate::ops::backup`].
//! The path variants back the native save/open dialogs; the `_text` variants move
//! the same JSON as a string (registered for parity with the web transport, 02 §10).

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::ops::backup as ops;
use crate::state::AppState;
use crate::storage::backup::{ImportMode, ImportResult};

#[tauri::command]
pub async fn export_backup(state: State<'_, Arc<AppState>>, path: String) -> Result<(), AppError> {
    ops::export_backup(state.inner(), path).await
}

#[tauri::command]
pub async fn import_backup(
    state: State<'_, Arc<AppState>>,
    path: String,
    mode: ImportMode,
) -> Result<ImportResult, AppError> {
    ops::import_backup(state.inner(), path, mode).await
}

#[tauri::command]
pub fn export_backup_text(state: State<'_, Arc<AppState>>) -> Result<String, AppError> {
    ops::export_backup_text(state.inner())
}

#[tauri::command]
pub async fn import_backup_text(
    state: State<'_, Arc<AppState>>,
    content: String,
    mode: ImportMode,
) -> Result<ImportResult, AppError> {
    ops::import_backup_text(state.inner(), content, mode).await
}
