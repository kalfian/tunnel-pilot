//! Group & tag commands (spec 02 §6.2): CRUD, assign, per-group start/stop,
//! list_tags. Per-group bulk is distinct from global `start_all`/`stop_all`
//! (AGENTS.md §1) — these act only on the forwards in one folder.
//!
//! Handlers stay thin (AGENTS §3): mutate `AppState` (RAM) →
//! `persist_groups`/`persist_forwards().await?` → emit the change event.

use std::sync::Arc;

use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::ssh::engine;
use crate::state::models::{ForwardStatus, GroupInput, TunnelGroup};
use crate::state::AppState;

/// `list_groups` — all groups (boot / rehydrate).
#[tauri::command]
pub fn list_groups(state: State<'_, Arc<AppState>>) -> Vec<TunnelGroup> {
    state.groups_snapshot()
}

/// `create_group` — append a new group with a fresh uuid; `order` is the next
/// index after the current highest so new folders sort last.
#[tauri::command]
pub async fn create_group(
    state: State<'_, Arc<AppState>>,
    input: GroupInput,
) -> Result<TunnelGroup, AppError> {
    let state = state.inner();
    let next_order = state
        .groups_snapshot()
        .iter()
        .map(|g| g.order)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);
    let group = TunnelGroup {
        id: Uuid::new_v4().to_string(),
        name: input.name,
        color: input.color,
        order: next_order,
        collapsed: input.collapsed,
    };
    state.upsert_group(group.clone());
    state.persist_groups().await?;
    state.emit_groups_changed();
    Ok(group)
}

/// `update_group` — rename/recolor and persist the `collapsed` flag (toggling a
/// folder open/closed survives restarts — spec 04 §2). `order` is preserved.
#[tauri::command]
pub async fn update_group(
    state: State<'_, Arc<AppState>>,
    id: String,
    input: GroupInput,
) -> Result<TunnelGroup, AppError> {
    let state = state.inner();
    let existing = state
        .groups_snapshot()
        .into_iter()
        .find(|g| g.id == id)
        .ok_or_else(|| AppError::NotFound(format!("group {id}")))?;

    let group = TunnelGroup {
        id,
        name: input.name,
        color: input.color,
        order: existing.order,
        collapsed: input.collapsed,
    };
    state.upsert_group(group.clone());
    state.persist_groups().await?;
    state.emit_groups_changed();
    Ok(group)
}

/// `delete_group` — remove the group; every forward assigned to it has its
/// `group_id` cleared (tags are kept) so nothing is orphaned (spec 02 §6.2).
#[tauri::command]
pub async fn delete_group(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    let state = state.inner();
    if !state.remove_group(&id) {
        return Err(AppError::NotFound(format!("group {id}")));
    }

    // Detach forwards from the deleted folder.
    let mut forwards_changed = false;
    for mut cfg in state.configs_snapshot() {
        if cfg.group_id.as_deref() == Some(id.as_str()) {
            cfg.group_id = None;
            state.upsert_config(cfg);
            forwards_changed = true;
        }
    }

    state.persist_groups().await?;
    state.emit_groups_changed();
    if forwards_changed {
        state.persist_forwards().await?;
        state.emit_forwards_changed();
    }
    Ok(())
}

/// `assign_forward_group` — move a forward into a folder (or `None` = ungrouped).
#[tauri::command]
pub async fn assign_forward_group(
    state: State<'_, Arc<AppState>>,
    forward_id: String,
    group_id: Option<String>,
) -> Result<(), AppError> {
    let state = state.inner();
    let mut cfg = state
        .get_config(&forward_id)
        .ok_or_else(|| AppError::NotFound(format!("forward {forward_id}")))?;

    // Reject an unknown target group so a forward never points at a dead folder.
    if let Some(gid) = &group_id {
        if !state.groups_snapshot().iter().any(|g| &g.id == gid) {
            return Err(AppError::NotFound(format!("group {gid}")));
        }
    }

    cfg.group_id = group_id;
    state.upsert_config(cfg);
    state.persist_forwards().await?;
    state.emit_forwards_changed();
    Ok(())
}

/// `start_group` — connect every forward in the folder (per-group bulk). Reuses
/// the same start policy as the global sweep but scoped to one folder.
#[tauri::command]
pub async fn start_group(
    state: State<'_, Arc<AppState>>,
    group_id: String,
) -> Result<(), AppError> {
    let state = state.inner();
    for cfg in state.configs_snapshot() {
        if cfg.group_id.as_deref() != Some(group_id.as_str()) {
            continue;
        }
        match state.registry.current_status(&cfg.id) {
            Some(ForwardStatus::Connected)
            | Some(ForwardStatus::Connecting)
            | Some(ForwardStatus::Disconnecting) => {}
            Some(ForwardStatus::Error) => {
                if let Err(e) = engine::retry_forward(state, &cfg.id).await {
                    tracing::error!(tunnel = %cfg.id, error = %e, "start_group: retry failed");
                }
            }
            Some(ForwardStatus::Disconnected) | None => {
                if let Err(e) = engine::connect_forward(state, &cfg.id).await {
                    tracing::error!(tunnel = %cfg.id, error = %e, "start_group: connect failed");
                }
            }
        }
    }
    Ok(())
}

/// `stop_group` — disconnect every live forward in the folder (per-group bulk).
#[tauri::command]
pub async fn stop_group(state: State<'_, Arc<AppState>>, group_id: String) -> Result<(), AppError> {
    let state = state.inner();
    // Snapshot ids first (the live set is scoped to this folder's forwards).
    let ids: Vec<String> = state
        .configs_snapshot()
        .into_iter()
        .filter(|c| c.group_id.as_deref() == Some(group_id.as_str()))
        .map(|c| c.id)
        .collect();
    for id in ids {
        if state.registry.contains(&id) {
            if let Err(e) = engine::disconnect_forward(state, &id, true).await {
                tracing::error!(tunnel = %id, error = %e, "stop_group: disconnect failed");
            }
        }
    }
    Ok(())
}

/// `list_tags` — the derived union of every forward's tags, sorted + de-duped
/// (spec 04 §2 — tags are free-form strings on the forwards).
#[tauri::command]
pub fn list_tags(state: State<'_, Arc<AppState>>) -> Vec<String> {
    let mut tags: Vec<String> = state
        .configs_snapshot()
        .into_iter()
        .flat_map(|c| c.tags)
        .collect();
    tags.sort();
    tags.dedup();
    tags
}
