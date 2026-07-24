//! Persistence: JSON config (atomic read-merge-write), v1→v2 migration, and
//! backup format (spec 03 §7, 04 §§9,11,12).
//!
//! - [`config_file`] — atomic read-merge-write of `tunnel_pilot_config.json`
//!   (M2, done).
//! - [`migration`] — hardcoded per-OS v1-path probe + import (M2, done).
//! - [`backup`] — backup wire format + lenient v1 parse (M2). Applying an
//!   import to `AppState` (replace|merge) lands with the command surface (M4).

pub mod backup;
pub mod config_file;
pub mod migration;
