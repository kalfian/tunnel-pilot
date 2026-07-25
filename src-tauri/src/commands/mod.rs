//! IPC command handlers (`#[tauri::command]`), the source-of-truth contract
//! (spec 02 §6, AGENTS.md §1). Handlers stay thin: parse args → call a service
//! in `ssh/`/`storage/`/`credentials/`/etc.
//!
//! Every command has a matching typed wrapper in `src/lib/ipc.ts`. The Rust
//! command, its `invoke_handler` registration, `lib/ipc.ts`, `lib/types.ts`, and
//! the spec 02 tables are kept in lockstep (AGENTS.md §1). The full M4 surface is
//! registered in one `tauri::generate_handler![...]` list in `lib.rs`.

pub mod app;
pub mod backup;
pub mod forwards;
pub mod groups;
pub mod logs;
pub mod settings;
pub mod updater;
