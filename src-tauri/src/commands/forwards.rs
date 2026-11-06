//! Forward commands (spec 02 §6.1): list/create/update/delete/duplicate,
//! reorder, connect/disconnect/retry, global `start_all`/`stop_all`,
//! `get_forward_runtime`, `copy_ssh_command`, set/clear password.
//!
//! Handlers stay thin (AGENTS §3): unwrap the Tauri `State` and call the shared
//! service function in [`crate::ops::forwards`] — the same one the tray and the
//! CLI control socket use. Passwords flow ONLY through `set_forward_password` /
//! `clear_forward_password` — never inside `ForwardInput` (spec 02 §6.1, §8).

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::ops::forwards as ops;
use crate::ssh::engine;
use crate::state::models::{ForwardConfig, ForwardInput, ForwardRuntime};
use crate::state::AppState;

// Re-exported for the desktop-only callers (tray, backup import) that already
// reach the bulk actions through this module.
pub use crate::ops::forwards::{run_start_all, run_stop_all};

// --- CRUD ---

/// `list_forwards` — display-ordered config list (boot / rehydrate).
#[tauri::command]
pub fn list_forwards(state: State<'_, Arc<AppState>>) -> Vec<ForwardConfig> {
    state.configs_snapshot()
}

/// `create_forward` — append a new forward (fresh uuid, no stored password) and
/// persist. The password (if any) is a separate `set_forward_password` call.
#[tauri::command]
pub async fn create_forward(
    state: State<'_, Arc<AppState>>,
    input: ForwardInput,
) -> Result<ForwardConfig, AppError> {
    ops::create(state.inner(), input).await
}

/// `update_forward` — edit an existing forward. If it is currently live it is
/// force-disconnected first (v1 parity), then the config is replaced preserving
/// its id and `has_stored_password` (the secret is untouched).
#[tauri::command]
pub async fn update_forward(
    state: State<'_, Arc<AppState>>,
    id: String,
    input: ForwardInput,
) -> Result<ForwardConfig, AppError> {
    ops::update(state.inner(), id, input).await
}

/// `delete_forward` — tear down the tunnel if live, drop its stored secret, then
/// remove the config and persist.
#[tauri::command]
pub async fn delete_forward(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    ops::delete(state.inner(), &id).await
}

/// `duplicate_forward` — copy a forward with a new id and " (copy)" suffix. The
/// duplicate never inherits the original's stored password (spec 02 §6.1).
#[tauri::command]
pub async fn duplicate_forward(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<ForwardConfig, AppError> {
    ops::duplicate(state.inner(), &id).await
}

/// `reorder_forwards` — set the display order to `ordered_ids` (see
/// [`ops::reorder`] for the omitted/unknown-id rules).
#[tauri::command]
pub async fn reorder_forwards(
    state: State<'_, Arc<AppState>>,
    ordered_ids: Vec<String>,
) -> Result<(), AppError> {
    ops::reorder(state.inner(), &ordered_ids).await
}

// --- Connection control (thin wrappers over the engine) ---

/// `connect_forward` — start (or restart) the tunnel's supervisor.
#[tauri::command]
pub async fn connect_forward(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    engine::connect_forward(state.inner(), &id).await
}

/// `disconnect_forward` — user-initiated (silent) disconnect.
#[tauri::command]
pub async fn disconnect_forward(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), AppError> {
    engine::disconnect_forward(state.inner(), &id, true).await
}

/// `retry_forward` — retry a tunnel parked in `error`.
#[tauri::command]
pub async fn retry_forward(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    engine::retry_forward(state.inner(), &id).await
}

/// `get_forward_runtime` — on-demand status+stats+lastError snapshot.
#[tauri::command]
pub fn get_forward_runtime(state: State<'_, Arc<AppState>>, id: String) -> ForwardRuntime {
    ops::runtime_or_default(state.inner(), &id)
}

/// `copy_ssh_command` — the `ssh -N -L ...` string for a forward (no password).
#[tauri::command]
pub fn copy_ssh_command(state: State<'_, Arc<AppState>>, id: String) -> Result<String, AppError> {
    let cfg = state
        .get_config(&id)
        .ok_or_else(|| AppError::NotFound(format!("forward {id}")))?;
    Ok(ops::build_ssh_command(&cfg))
}

// --- Credentials (route through the keychain / fallback store) ---

/// `set_forward_password` — store the secret in the keychain/fallback and flip
/// `has_stored_password`. The password is NEVER logged/emitted (AGENTS §8).
#[tauri::command]
pub async fn set_forward_password(
    state: State<'_, Arc<AppState>>,
    id: String,
    password: String,
) -> Result<(), AppError> {
    ops::set_password(state.inner(), &id, password).await
}

/// `clear_forward_password` — remove the stored secret and clear the flag.
#[tauri::command]
pub async fn clear_forward_password(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), AppError> {
    ops::clear_password(state.inner(), &id).await
}

// --- Global bulk (F3) — distinct from per-group start/stop (AGENTS §1) ---

/// `start_all` command (spec 02 §6.1) — global bulk connect.
#[tauri::command]
pub async fn start_all(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    run_start_all(state.inner()).await
}

/// `stop_all` command (spec 02 §6.1) — global bulk disconnect (user-initiated).
#[tauri::command]
pub async fn stop_all(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    run_stop_all(state.inner()).await
}
