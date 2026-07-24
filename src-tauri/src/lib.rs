//! Tunnel Pilot v2 core library (Tauri v2 + tokio).
//!
//! This is the always-alive core process: it owns the tray, the tokio runtime,
//! all SSH tunnels, persistence, keychain, updater, and app lifecycle. The
//! webview/frontend is pure presentation and may be torn down when hidden.
//! See `spec/02-ARCHITECTURE.md` for the full architecture.
//!
//! Module tree, plugins, tray, and tracing are wired across the M0 items.

// M0 scaffold: the subsystem modules below are stubs (doc comments + TODO
// markers) filled in M1+. Their public items are intentionally not yet
// referenced, so allow dead_code crate-wide during the scaffold phase. Remove
// this once the engine/commands wire the modules up (M1/M4).
#![allow(dead_code)]

pub mod commands;
pub mod credentials;
pub mod error;
pub mod events;
pub mod platform;
pub mod ssh;
pub mod state;
pub mod storage;
pub mod tray;
pub mod updater;
pub mod window;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

/// Build and run the Tauri application.
///
/// The window starts hidden (`visible: false` in `tauri.conf.json`) so the app
/// boots straight into the tray.
pub fn run() {
    tauri::Builder::default()
        // single-instance MUST be registered first (spec 02 §8). On a second
        // launch it re-shows the window; M3 wires show_window + window://focus.
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {
            // TODO(M3): show_window() + emit WINDOW_FOCUS.
        }))
        // Launch-at-login. Reconciled with the `launchAtLogin` setting in M3.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        // Updater endpoints/pubkey are configured in M6 (minisign signing).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // macOS: sit in the tray as an agent app (baseline; the `showInDock`
            // activation-policy switching lands in M3, spec 03 §13). Mirrors the
            // Info.plist LSUIElement flag so dev runs also stay dock-less.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Minimal tray: Open (show/focus the window) + Quit (exit). The full
            // dynamic icon + rebuilt menu lands in M3 (spec 03 §§12,13).
            let open_item = MenuItem::with_id(app, "open", "Open", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("Tunnel Pilot")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                });

            // Reuse the bundled app icon for the tray when available.
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            // Tracing→log-buffer layer is initialized in the next M0 item.
            Ok(())
        })
        // `expect` is acceptable at this binary edge: a failure here means the
        // app cannot start at all (AGENTS.md §4 — provably-terminal, bin edge).
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
