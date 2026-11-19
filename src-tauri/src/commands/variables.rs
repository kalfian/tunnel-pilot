//! Variable commands (spec 02 §6.9): `list_variables`, `save_variable`,
//! `delete_variable`. Thin wrappers over [`crate::ops::variables`]. A secret
//! value only ever travels inbound (in `save_variable`'s input); no command
//! returns one (AGENTS §8).

use std::sync::Arc;

use tauri::State;

use crate::error::AppError;
use crate::ops::variables as ops;
use crate::state::models::{Variable, VariableInput};
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
