//! Forward commands (spec 02 §6.1): list/create/update/delete/duplicate,
//! reorder, connect/disconnect/retry, global `start_all`/`stop_all`,
//! `get_forward_runtime`, `copy_ssh_command`, set/clear password.
//!
//! M3 implements the global bulk commands `start_all`/`stop_all` (F3) and wires
//! them to the tray. The rest (CRUD/reorder/copy-ssh/passwords) lands in M4.
//! Passwords will flow ONLY via set/clear (never in `ForwardInput`).

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::ssh::engine;
use crate::state::models::ForwardStatus;
use crate::state::AppState;

/// Global bulk connect (v1 `connectAll`, F3): connect every configured forward
/// that is not already connected/connecting; retry any parked in `error`.
///
/// Distinct from per-group `start_group` (spec 02 §6.2, AGENTS §1) — this is the
/// global tray/palette/keymap action. Best-effort: a per-tunnel failure is
/// logged inside the engine and does not abort the sweep.
pub async fn run_start_all(state: &Arc<AppState>) -> Result<(), AppError> {
    for cfg in state.configs_snapshot() {
        match state.registry.current_status(&cfg.id) {
            // Already live and healthy/transitioning — leave it.
            Some(ForwardStatus::Connected)
            | Some(ForwardStatus::Connecting)
            | Some(ForwardStatus::Disconnecting) => {}
            // Parked in error — reuse the supervisor via retry.
            Some(ForwardStatus::Error) => {
                if let Err(e) = engine::retry_forward(state, &cfg.id).await {
                    tracing::error!(tunnel = %cfg.id, error = %e, "start_all: retry failed");
                }
            }
            // Disconnected or not live — start a fresh supervisor.
            Some(ForwardStatus::Disconnected) | None => {
                if let Err(e) = engine::connect_forward(state, &cfg.id).await {
                    tracing::error!(tunnel = %cfg.id, error = %e, "start_all: connect failed");
                }
            }
        }
    }
    Ok(())
}

/// Global bulk disconnect (v1 `disconnectAll`, F3): user-initiated (silent)
/// disconnect of every live tunnel. Distinct from per-group `stop_group`.
pub async fn run_stop_all(state: &Arc<AppState>) -> Result<(), AppError> {
    for id in state.registry.all_ids() {
        if let Err(e) = engine::disconnect_forward(state, &id, true).await {
            tracing::error!(tunnel = %id, error = %e, "stop_all: disconnect failed");
        }
    }
    Ok(())
}

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
