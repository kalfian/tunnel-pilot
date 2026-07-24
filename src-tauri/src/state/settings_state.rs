//! Settings state.
//!
//! The `AppSettings` RAM mirror + its persistence live directly on
//! [`crate::state::AppState`] (`settings_snapshot`/`set_settings`, backed by
//! `storage::config_file::ConfigStore`). Change propagation over
//! `settings://changed` lands with the settings command surface (M4); this
//! module is kept as a placeholder for that wiring.
