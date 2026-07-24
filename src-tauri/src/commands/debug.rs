//! TEMPORARY debug/test-only commands to drive the M1 SSH engine before the
//! real command surface lands (M4). These are thin wrappers over
//! `ssh::engine` + `AppState` and are expected to be removed/replaced when
//! `commands/forwards.rs` is implemented. Kept intentionally minimal.

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::ssh::engine;
use crate::state::models::{ForwardConfig, ForwardRuntime};
use crate::state::AppState;

/// Inject/replace a config in the in-memory store (M1 stand-in for persistence).
#[tauri::command]
pub fn debug_upsert_config(state: State<'_, Arc<AppState>>, config: ForwardConfig) {
    state.upsert_config(config);
}

/// Store a password in the in-memory credential stand-in (M1; M2 = keychain).
#[tauri::command]
pub fn debug_set_password(state: State<'_, Arc<AppState>>, id: String, password: String) {
    state.set_password(&id, password);
}

/// Start a tunnel (launches its supervisor).
#[tauri::command]
pub async fn debug_connect(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    engine::connect_forward(state.inner(), &id).await
}

/// User-initiated disconnect.
#[tauri::command]
pub async fn debug_disconnect(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    engine::disconnect_forward(state.inner(), &id, true).await
}

/// Retry a tunnel parked in `error`.
#[tauri::command]
pub async fn debug_retry(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    engine::retry_forward(state.inner(), &id).await
}

/// Snapshot the current runtime (status + stats + last error).
#[tauri::command]
pub fn debug_runtime(state: State<'_, Arc<AppState>>, id: String) -> Option<ForwardRuntime> {
    state.registry.runtime(&id)
}
