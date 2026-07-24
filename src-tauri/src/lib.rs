//! Tunnel Pilot v2 core library (Tauri v2 + tokio).
//!
//! This is the always-alive core process: it owns the tray, the tokio runtime,
//! all SSH tunnels, persistence, keychain, updater, and app lifecycle. The
//! webview/frontend is pure presentation and may be torn down when hidden.
//! See `spec/02-ARCHITECTURE.md` for the full architecture.
//!
//! Module tree, plugins, tray, and tracing are wired across the M0 items.

/// Build and run the Tauri application.
///
/// The window starts hidden (`visible: false` in `tauri.conf.json`) so the app
/// boots straight into the tray.
pub fn run() {
    tauri::Builder::default()
        .setup(|_app| {
            // Tray, plugins, and the tracing→log-buffer layer are registered in
            // later M0 items. Window is hidden at start via tauri.conf.json.
            Ok(())
        })
        // `expect` is acceptable at this binary edge: a failure here means the
        // app cannot start at all (AGENTS.md §4 — provably-terminal, bin edge).
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
