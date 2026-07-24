//! In-memory ring buffer of `LogEntry` (cap 500, newest-first). Not persisted.
//!
//! Backs the Logs tab; fed by the tracing layer + explicit `log()` helper, and
//! emits `log://line` / `log://cleared` (spec 03 §18).
//!
//! TODO(M2): `LogBuffer` (VecDeque cap 500), snapshot/clear, formatted text.
//! The tracing→buffer layer stub is initialized in `logging.rs` (M0 item 7).
