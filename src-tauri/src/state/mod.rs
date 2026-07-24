//! Application state — the source of truth for the app (spec 02 §5).
//!
//! `AppState` (added in M1/M2) is wired as `tauri::State` and owns the config
//! list, settings mirror, tunnel registry, and log buffer.
//!
//! TODO(M1/M2): `AppState` struct + accessors.

pub mod log_buffer;
pub mod settings_state;
pub mod tunnel_registry;
