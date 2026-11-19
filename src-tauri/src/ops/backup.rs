//! Backup service fns (spec 02 §6.5), Tauri-free. The desktop app exports to and
//! imports from a file path the user picked in a native dialog; the web UI (02 §10)
//! cannot name a server path, so it moves the same JSON as text instead. Both paths
//! share one export and one import implementation. Passwords are never part of a
//! backup (spec 04 §10 — `BackupFile::export` strips them).

use std::sync::Arc;

use crate::error::AppError;
use crate::state::AppState;
use crate::storage::backup::{
    plan_import, plan_variable_import, BackupFile, ImportMode, ImportResult,
};
use crate::variables::secret_account;

/// The backup document for the current config, as pretty JSON.
pub fn export_backup_text(state: &Arc<AppState>) -> Result<String, AppError> {
    BackupFile::export(
        &state.configs_snapshot(),
        &state.groups_snapshot(),
        &state.variables_snapshot(),
        chrono::Utc::now().to_rfc3339(),
    )
    .to_json()
}

/// `export_backup` — write the backup to `path` (desktop save dialog).
pub async fn export_backup(state: &Arc<AppState>, path: String) -> Result<(), AppError> {
    let json = export_backup_text(state)?;
    tokio::fs::write(&path, json)
        .await
        .map_err(|e| AppError::Backup(format!("failed to write backup to {path}: {e}")))?;
    tracing::info!(path = %path, "exported backup");
    Ok(())
}

/// `import_backup` — read a backup from `path` (desktop open dialog) and apply it.
pub async fn import_backup(
    state: &Arc<AppState>,
    path: String,
    mode: ImportMode,
) -> Result<ImportResult, AppError> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| AppError::Backup(format!("failed to read backup from {path}: {e}")))?;
    import_backup_bytes(state, &bytes, mode).await
}

/// `import_backup_text` — apply a backup uploaded as text (web UI).
pub async fn import_backup_text(
    state: &Arc<AppState>,
    content: String,
    mode: ImportMode,
) -> Result<ImportResult, AppError> {
    import_backup_bytes(state, content.as_bytes(), mode).await
}

/// Validate + plan + apply. A `replace` import stops every live tunnel first so
/// no supervisor keeps running against a config that no longer exists.
async fn import_backup_bytes(
    state: &Arc<AppState>,
    bytes: &[u8],
    mode: ImportMode,
) -> Result<ImportResult, AppError> {
    let mut backup = crate::storage::backup::parse_backup(bytes)?;
    // Serialize with variable saves/deletes so neither undoes the other.
    let _guard = state.lock_variables().await;
    let (variables, orphaned_secrets) = plan_variable_import(
        &state.variables_snapshot(),
        std::mem::take(&mut backup.variables),
        mode,
    );
    let (forwards, groups, result) = plan_import(
        &state.configs_snapshot(),
        &state.groups_snapshot(),
        backup,
        mode,
    );

    if result.replaced {
        crate::ops::forwards::run_stop_all(state).await?;
    }

    // Variables first, so no forward ever references one that is not there.
    state.replace_variables(variables);
    state.replace_configs(forwards);
    state.replace_groups(groups);
    state.persist_variables().await?;
    state.persist_forwards().await?;
    state.persist_groups().await?;
    state.emit_variables_changed();
    state.emit_forwards_changed();
    state.emit_groups_changed();
    // Secrets of variables the import dropped (best-effort: the config no
    // longer references them either way).
    // Bindings the import left without a forward go (spec 03 §24).
    crate::ops::variables::prune_bindings_locked(state).await?;
    for name in orphaned_secrets {
        if let Err(e) = state.delete_password_checked(&secret_account(&name)).await {
            tracing::warn!(variable = %name, error = %e, "could not delete a dropped variable's secret");
        }
    }

    tracing::info!(
        imported = result.imported,
        skipped = result.skipped,
        replaced = result.replaced,
        "imported backup"
    );
    Ok(result)
}
