//! Tauri-free service layer shared by every driver of `AppState`: the webview's
//! `#[tauri::command]` wrappers, the tray, the CLI control socket and the web
//! transport (spec 02 §5, §10; 03 §21–22). One implementation per capability.

pub mod backup;
pub mod dispatch;
pub mod forwards;
pub mod groups;
pub mod logs;
pub mod settings;
