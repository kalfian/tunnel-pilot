//! Group & tag commands (spec 02 §6.2): CRUD, assign, per-group start/stop,
//! list_tags. Per-group bulk is distinct from global `start_all`/`stop_all`
//! (AGENTS.md §1) — these act only on the forwards in one folder.
//!
//! Handlers stay thin (AGENTS §3): mutate `AppState` (RAM) →
//! `persist_groups`/`persist_forwards().await?` → emit the change event.

use crate::error::AppError;
use crate::state::models::{GroupInput, TunnelGroup};
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

use crate::ops::groups as ops;

#[tauri::command]
pub fn list_groups(state: State<'_, Arc<AppState>>) -> Vec<TunnelGroup> {
    ops::list_groups(state.inner())
}

#[tauri::command]
pub async fn create_group(
    state: State<'_, Arc<AppState>>,
    input: GroupInput,
) -> Result<TunnelGroup, AppError> {
    ops::create_group(state.inner(), input).await
}

#[tauri::command]
pub async fn update_group(
    state: State<'_, Arc<AppState>>,
    id: String,
    input: GroupInput,
) -> Result<TunnelGroup, AppError> {
    ops::update_group(state.inner(), id, input).await
}

#[tauri::command]
pub async fn delete_group(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    ops::delete_group(state.inner(), id).await
}

#[tauri::command]
pub async fn assign_forward_group(
    state: State<'_, Arc<AppState>>,
    forward_id: String,
    group_id: Option<String>,
) -> Result<(), AppError> {
    ops::assign_forward_group(state.inner(), forward_id, group_id).await
}

#[tauri::command]
pub async fn start_group(
    state: State<'_, Arc<AppState>>,
    group_id: String,
) -> Result<(), AppError> {
    ops::start_group(state.inner(), group_id).await
}

#[tauri::command]
pub async fn stop_group(state: State<'_, Arc<AppState>>, group_id: String) -> Result<(), AppError> {
    ops::stop_group(state.inner(), group_id).await
}

#[tauri::command]
pub fn list_tags(state: State<'_, Arc<AppState>>) -> Vec<String> {
    ops::list_tags(state.inner())
}
