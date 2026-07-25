//! Logs commands (spec 02 §6.4): `get_logs`, `clear_logs`, `get_logs_text`
//! (formatted lines for Copy All). Thin wrappers over the shared
//! `state/log_buffer.rs` ring buffer (spec 03 §18).

use std::sync::Arc;

use tauri::State;

use crate::state::models::LogEntry;
use crate::state::AppState;

/// `get_logs` — snapshot of the ring buffer, newest-first.
#[tauri::command]
pub fn get_logs(state: State<'_, Arc<AppState>>) -> Vec<LogEntry> {
    state.log_buffer().snapshot()
}

/// `clear_logs` — empty the buffer and emit `log://cleared`.
#[tauri::command]
pub fn clear_logs(state: State<'_, Arc<AppState>>) {
    state.log_buffer().clear();
}

/// `get_logs_text` — the joined formatted lines for Copy All (spec 04 §6).
#[tauri::command]
pub fn get_logs_text(state: State<'_, Arc<AppState>>) -> String {
    state.log_buffer().formatted_text()
}
