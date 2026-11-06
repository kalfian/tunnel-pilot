//! Tauri-free service layer shared by every driver of `AppState` (webview
//! commands, tray, CLI control socket). Compiled in both the desktop and the
//! headless CLI build (spec 02 §5, 03 §21).

pub mod forwards;
