//! Log service fns (spec 02 §6.4), Tauri-free (shared by every transport).

use std::sync::Arc;

use crate::state::models::LogEntry;
use crate::state::AppState;

/// `get_logs` — snapshot of the ring buffer, newest-first.
pub fn get_logs(state: &Arc<AppState>) -> Vec<LogEntry> {
    state.log_buffer().snapshot()
}

/// `clear_logs` — empty the buffer and emit `log://cleared`.
pub fn clear_logs(state: &Arc<AppState>) {
    state.log_buffer().clear();
}

/// `get_logs_text` — the joined formatted lines for Copy All (spec 04 §6).
pub fn get_logs_text(state: &Arc<AppState>) -> String {
    state.log_buffer().formatted_text()
}
