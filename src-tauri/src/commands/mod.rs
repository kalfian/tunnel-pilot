//! IPC command handlers (`#[tauri::command]`), the source-of-truth contract
//! (spec 02 §6, AGENTS.md §1). Handlers stay thin: parse args → call a service
//! in `ssh/`/`storage/`/`credentials/`/etc.
//!
//! Every command has a matching typed wrapper in `src/lib/ipc.ts`. As commands
//! land (M3/M4+), register them in one `tauri::generate_handler![...]` list and
//! keep Rust + `lib/ipc.ts` + `lib/types.ts` + spec 02 tables in lockstep.
//!
//! TODO(M3/M4): implement + register the commands catalogued in spec 02 §6.

pub mod app;
pub mod backup;
pub mod forwards;
pub mod groups;
pub mod logs;
pub mod settings;
pub mod updater;
