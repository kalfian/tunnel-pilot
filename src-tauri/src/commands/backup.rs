//! Backup commands (spec 02 §6.5): `export_backup` (strips passwords),
//! `import_backup` (validate, reject version>current, replace|merge).
//!
//! Export writes a password-free [`BackupFile`] (AGENTS §8); import reads +
//! validates a v1/v2 backup (F19 lenient), plans the merge via the pure
//! [`plan_import`], applies it to `AppState`, persists (F37 surfaces failures),
//! and emits the change events. On a REPLACE, live tunnels are stopped first so
//! none is left bound to a port whose config just vanished.

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::state::AppState;
use crate::storage::backup::{plan_import, BackupFile, ImportMode, ImportResult};

/// `export_backup` — write the current forwards+groups to `path` as a v2 backup,
/// with every `hasStoredPassword` forced false and no secret on the wire.
#[tauri::command]
pub async fn export_backup(state: State<'_, Arc<AppState>>, path: String) -> Result<(), AppError> {
    let state = state.inner();
    let backup = BackupFile::export(
        &state.configs_snapshot(),
        &state.groups_snapshot(),
        chrono::Utc::now().to_rfc3339(),
    );
    let json = backup.to_json()?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|e| AppError::Backup(format!("failed to write backup to {path}: {e}")))?;
    tracing::info!(path = %path, "exported backup");
    Ok(())
}

/// `import_backup` — read + validate a backup at `path`, apply it (replace or
/// merge), persist, and return the [`ImportResult`]. Rejects a backup whose
/// version is newer than this app supports (done in `parse_backup`).
#[tauri::command]
pub async fn import_backup(
    state: State<'_, Arc<AppState>>,
    path: String,
    mode: ImportMode,
) -> Result<ImportResult, AppError> {
    let state = state.inner();
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| AppError::Backup(format!("failed to read backup from {path}: {e}")))?;
    let backup = crate::storage::backup::parse_backup(&bytes)?;

    let (forwards, groups, result) = plan_import(
        &state.configs_snapshot(),
        &state.groups_snapshot(),
        backup,
        mode,
    );

    // A REPLACE removes configs; stop every live tunnel first so nothing is left
    // bound to a port whose config just disappeared.
    if result.replaced {
        crate::commands::forwards::run_stop_all(state).await?;
    }

    state.replace_configs(forwards);
    state.replace_groups(groups);
    state.persist_forwards().await?;
    state.persist_groups().await?;
    state.emit_forwards_changed();
    state.emit_groups_changed();

    tracing::info!(
        imported = result.imported,
        skipped = result.skipped,
        replaced = result.replaced,
        "imported backup"
    );
    Ok(result)
}
