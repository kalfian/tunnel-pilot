//! Persistence: JSON config (atomic read-merge-write), v1→v2 migration, and
//! backup export/import (spec 03 §7, 04 §§9,11,12).
//!
//! TODO(M2/M4): fill the submodules below.

pub mod backup;
pub mod config_file;
pub mod migration;
