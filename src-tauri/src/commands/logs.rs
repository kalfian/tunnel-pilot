//! Logs commands (spec 02 §6.4): `get_logs`, `clear_logs`, `get_logs_text`
//! (formatted lines for Copy All). Thin wrappers over the shared
//! `state/log_buffer.rs` ring buffer (spec 03 §18).

use crate::state::models::LogEntry;
use crate::state::AppState;
use std::sync::Arc;
use tauri::State;

use crate::ops::logs as ops;

#[tauri::command]
pub fn get_logs(state: State<'_, Arc<AppState>>) -> Vec<LogEntry> {
    ops::get_logs(state.inner())
}

#[tauri::command]
pub fn clear_logs(state: State<'_, Arc<AppState>>) {
    ops::clear_logs(state.inner())
}

#[tauri::command]
pub fn get_logs_text(state: State<'_, Arc<AppState>>) -> String {
    ops::get_logs_text(state.inner())
}
