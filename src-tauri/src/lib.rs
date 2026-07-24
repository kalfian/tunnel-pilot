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
pub mod logging;
pub mod platform;
pub mod ssh;
pub mod state;
pub mod storage;
pub mod tray;
pub mod updater;
pub mod window;

use std::sync::Arc;

use tauri::Manager;

use crate::credentials::CredentialStore;
use crate::state::AppState;
use crate::storage::config_file::{ConfigDocument, ConfigStore};

/// Build and run the Tauri application.
///
/// The window starts hidden (`visible: false` in `tauri.conf.json`) so the app
/// boots straight into the tray.
pub fn run() {
    tauri::Builder::default()
        // single-instance MUST be registered first (spec 02 §8/§11). On a second
        // launch its callback runs in the already-running instance and re-shows +
        // focuses the window, emitting `window://focus` for the frontend.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            crate::window::focus_from_second_instance(app);
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
        // M1: temporary debug commands to drive the SSH engine (replaced at M4).
        .invoke_handler(tauri::generate_handler![
            crate::commands::debug::debug_upsert_config,
            crate::commands::debug::debug_set_password,
            crate::commands::debug::debug_connect,
            crate::commands::debug::debug_disconnect,
            crate::commands::debug::debug_retry,
            crate::commands::debug::debug_runtime,
            crate::commands::debug::debug_hydrate,
            // M3: global bulk connect/disconnect (spec 02 §6.1, F3).
            crate::commands::forwards::start_all,
            crate::commands::forwards::stop_all,
        ])
        .setup(|app| {
            // Initialize tracing + the (stubbed) tracing→log-buffer layer first
            // so subsequent setup steps are captured (spec 03 §18).
            crate::logging::init_tracing();
            tracing::info!("Tunnel Pilot v{} starting", env!("CARGO_PKG_VERSION"));

            // Persistence + credentials live in the single canonical v2 config
            // dir (`app_config_dir`, F2 — spec 03 §7). The keychain-fallback
            // secrets file lives in the same directory.
            let config_dir = app
                .path()
                .app_config_dir()
                .expect("app_config_dir must resolve");
            let config_store = Arc::new(ConfigStore::from_config_dir(&config_dir));
            let credentials = Arc::new(CredentialStore::from_app_dir(&config_dir));

            // First-run v1→v2 migration (hardcoded per-OS v1 probe, plaintext
            // passwords → keychain), then load the v2 document. Bad config never
            // crashes the app (spec 03 §7): on error we log and start with
            // defaults. `block_on` at this binary edge is fine — it is a one-
            // time boot step before the event loop spins up.
            let doc = tauri::async_runtime::block_on(async {
                if let Err(e) = crate::storage::migration::migrate_if_needed(
                    config_store.as_ref(),
                    credentials.as_ref(),
                )
                .await
                {
                    tracing::error!(error = %e, "v1→v2 migration failed; continuing");
                }
                config_store.load().await
            })
            .unwrap_or_else(|e| {
                tracing::error!(error = %e, "failed to load config; starting with defaults");
                ConfigDocument::default()
            });

            if !credentials.keychain_available() {
                tracing::warn!(
                    "OS keychain unavailable — SSH passwords use the plaintext fallback file; \
                     the UI will surface a warning (M4)"
                );
            }

            // The shared application state (source of truth, spec 02 §5). Owns
            // the tunnel registry + the persisted config mirror + credentials.
            let state = Arc::new(AppState::new_hydrated(
                app.handle().clone(),
                config_store,
                credentials,
                doc,
            ));
            app.manage(state.clone());

            // Sleep/wake watchdog (spec 03 §4): an app-lifetime monotonic-gap
            // task that probes connected tunnels after a >30s gap (likely OS
            // sleep). Best-effort; the russh session-future signal is the
            // backstop (F15). The 3s stats sampler (health.rs) is NOT started
            // here — it auto-starts on the first connect.
            crate::ssh::wake::spawn_wake_watchdog(state.clone());

            // macOS: sit in the tray as an agent app. The window boots hidden, so
            // the dock stays hidden regardless of `showInDock` (spec 03 §13) —
            // `window::show_window` flips to Regular later iff the setting is on.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // Autostart (spec 03 §12): reconcile the OS launch-at-login
            // registration with the persisted `launchAtLogin` setting on every
            // boot, correcting any drift.
            crate::platform::autostart::reconcile(
                &app.handle().clone(),
                state.settings_snapshot().launch_at_login,
            );

            // Full dynamic tray (spec 03 §§10,11): count icon 1–9, per-tunnel rows
            // with Retry-on-error, conditional bulk Start/Stop All, update-notice
            // slot; rebuilt (debounced) on `tunnel://status` changes.
            crate::tray::setup(app, state)?;

            // Hide-on-close intercept (spec 03 §14): the OS close button hides the
            // window and keeps the app alive in the tray; only Quit exits.
            crate::window::install_close_handler(&app.handle().clone());

            // Tracing→log-buffer layer is initialized in the next M0 item.
            Ok(())
        })
        // `expect` is acceptable at this binary edge: a failure here means the
        // app cannot start at all (AGENTS.md §4 — provably-terminal, bin edge).
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
