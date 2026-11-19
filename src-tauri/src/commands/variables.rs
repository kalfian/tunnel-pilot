//! Variable commands (spec 02 §6.9): `list_variables`, `save_variable`,
//! `delete_variable`. Thin wrappers over [`crate::ops::variables`]. A secret
//! value only ever travels inbound (in `save_variable`'s input); no command
//! returns one (AGENTS §8).

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::ops::variables as ops;
use crate::state::models::{Variable, VariableEndpointInput, VariableInput};
use crate::state::AppState;

/// `list_variables` — every variable, without secret values.
#[tauri::command]
pub fn list_variables(state: State<'_, Arc<AppState>>) -> Vec<Variable> {
    ops::list(state.inner())
}

/// `save_variable` — create a variable or change its value / kind.
#[tauri::command]
pub async fn save_variable(
    state: State<'_, Arc<AppState>>,
    input: VariableInput,
) -> Result<Variable, AppError> {
    ops::save(state.inner(), input).await
}

/// `delete_variable` — refused while a forward uses it.
#[tauri::command]
pub async fn delete_variable(
    state: State<'_, Arc<AppState>>,
    name: String,
) -> Result<(), AppError> {
    ops::delete(state.inner(), &name).await
}

/// `allow_variable_endpoint` — bind a secret variable to a server by proving
/// its value (the web UI's path; the desktop form binds implicitly).
#[tauri::command]
pub async fn allow_variable_endpoint(
    state: State<'_, Arc<AppState>>,
    input: VariableEndpointInput,
) -> Result<Variable, AppError> {
    // The desktop never needs a grant (it binds servers itself); the
    // check still runs so the command behaves the same everywhere.
    ops::allow_endpoint_with_proof(state.inner(), "desktop", input).await
}

/// `revoke_variable_endpoint` — stop sending a secret variable to a server.
#[tauri::command]
pub async fn revoke_variable_endpoint(
    state: State<'_, Arc<AppState>>,
    name: String,
    endpoint: String,
) -> Result<Variable, AppError> {
    ops::revoke_endpoint(state.inner(), &name, &endpoint).await
}
