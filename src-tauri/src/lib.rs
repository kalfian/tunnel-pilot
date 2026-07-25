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

/// CLI flag baked into the OS autostart registration (see the autostart plugin
/// init below). Present in argv ⇒ this launch was triggered at login ⇒ stay
/// hidden in the tray. Absent ⇒ a normal user launch ⇒ show + focus the window.
const AUTOSTART_ARG: &str = "--minimized";

/// True iff this process was started by the OS autostart/login registration,
/// detected via the [`AUTOSTART_ARG`] flag in argv (spec 03 §12).
fn launched_from_autostart() -> bool {
    std::env::args().any(|a| a == AUTOSTART_ARG)
}

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
        // The `--minimized` arg is baked into the OS autostart registration, so a
        // login-triggered launch carries it in argv while a normal user launch
        // does not — that's how boot decides to show vs stay hidden (spec 03 §12).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_notification::init())
        // Self-updater (spec 03 §16). Endpoints + the minisign public key live in
        // `tauri.conf.json` (`plugins.updater`); the private key is a CI secret
        // only. `check_update`/`install_update` drive it via `crate::updater`.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // Positioner: anchors the `tray_popover` window below the tray icon
        // (`Position::TrayBottomCenter`). The `tray-icon` feature's tray-rect
        // cache is fed by `on_tray_event` in the tray click handler.
        .plugin(tauri_plugin_positioner::init())
        // Full M4 command surface (spec 02 §6). Kept in lockstep with
        // `src/lib/ipc.ts` + `src/lib/types.ts` + the spec 02 tables (AGENTS §1);
        // the capabilities in `capabilities/default.json` scope these to the
        // main window (AGENTS §8).
        .invoke_handler(tauri::generate_handler![
            // Forwards (§6.1)
            crate::commands::forwards::list_forwards,
            crate::commands::forwards::create_forward,
            crate::commands::forwards::update_forward,
            crate::commands::forwards::delete_forward,
            crate::commands::forwards::duplicate_forward,
            crate::commands::forwards::reorder_forwards,
            crate::commands::forwards::connect_forward,
            crate::commands::forwards::disconnect_forward,
            crate::commands::forwards::retry_forward,
            crate::commands::forwards::start_all,
            crate::commands::forwards::stop_all,
            crate::commands::forwards::get_forward_runtime,
            crate::commands::forwards::copy_ssh_command,
            crate::commands::forwards::set_forward_password,
            crate::commands::forwards::clear_forward_password,
            // Groups & tags (§6.2)
            crate::commands::groups::list_groups,
            crate::commands::groups::create_group,
            crate::commands::groups::update_group,
            crate::commands::groups::delete_group,
            crate::commands::groups::assign_forward_group,
            crate::commands::groups::start_group,
            crate::commands::groups::stop_group,
            crate::commands::groups::list_tags,
            // Settings (§6.3)
            crate::commands::settings::get_settings,
            crate::commands::settings::update_settings,
            // Logs (§6.4)
            crate::commands::logs::get_logs,
            crate::commands::logs::clear_logs,
            crate::commands::logs::get_logs_text,
            // Backup (§6.5)
            crate::commands::backup::export_backup,
            crate::commands::backup::import_backup,
            // Updater (§6.6)
            crate::commands::updater::check_update,
            crate::commands::updater::install_update,
            crate::commands::updater::skip_update,
            // App / window (§6.7)
            crate::commands::app::app_hydrate,
            crate::commands::app::show_window,
            crate::commands::app::hide_window,
            crate::commands::app::quit_app,
            crate::commands::app::hide_tray_popover,
        ])
        .setup(|app| {
            // The log ring buffer must exist BEFORE tracing init so the layer
            // can write into it; the SAME Arc is shared with `AppState` below so
            // the `get_logs` command reads exactly what the layer writes (spec
            // 03 §18). The app handle is attached inside `new_hydrated` so
            // appends start emitting `log://line`.
            let logs = Arc::new(crate::state::log_buffer::LogBuffer::new());
            crate::logging::set_log_buffer(logs.clone());
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
                logs,
                doc,
            ));
            app.manage(state.clone());

            // Updater runtime state (spec 03 §16): holds the pending signed
            // bundle from the last check + once-per-version notification guard.
            // Managed alongside `AppState` so the updater commands and the boot
            // auto-check share one instance.
            let updater_state = Arc::new(crate::updater::UpdaterState::new());
            app.manage(updater_state.clone());

            // Boot update-check (spec 03 §16): one check at startup iff
            // `autoCheckUpdates`, firing the update-available notification once
            // per version and emitting `update://status`. Runs detached so a slow
            // or failing network check never blocks startup (errors swallowed).
            {
                let app_handle = app.handle().clone();
                let state_for_updater = state.clone();
                let updater_for_task = updater_state.clone();
                tauri::async_runtime::spawn(crate::updater::auto_check_on_startup(
                    app_handle,
                    state_for_updater,
                    updater_for_task,
                ));
            }

            // Sleep/wake watchdog (spec 03 §4): an app-lifetime monotonic-gap
            // task that probes connected tunnels after a >30s gap (likely OS
            // sleep). Best-effort; the russh session-future signal is the
            // backstop (F15). The 3s stats sampler (health.rs) is NOT started
            // here — it auto-starts on the first connect.
            crate::ssh::wake::spawn_wake_watchdog(state.clone());

            // macOS dock/activation policy follows the `showInDock` SETTING
            // (BUG 1), applied once here from the loaded setting and again on
            // every settings change — NOT tied to window visibility, so the dock
            // icon persists across window open/close (present iff showInDock is
            // on). No-op on Windows/Linux. `window::show_window` fronts the
            // window without changing this policy.
            crate::platform::dock::apply_dock_policy(
                &app.handle().clone(),
                state.settings_snapshot().show_in_dock,
            );

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

            // Tray popover window (`tray_popover`): a compact, borderless panel
            // shown on tray LEFT-click, anchored below the icon. Created hidden at
            // boot; loads the SAME index.html as the main window (the FE branches
            // on the window label). `PopoverState` guards blur-to-dismiss against
            // the opening-click's transient blur — managed so the blur handler and
            // the show path share one instance.
            let popover_state = Arc::new(crate::window::popover::PopoverState::new());
            app.manage(popover_state.clone());
            crate::window::popover::create_popover(&app.handle().clone(), popover_state)?;

            // Hide-on-close intercept (spec 03 §14): the OS close button hides the
            // window and keeps the app alive in the tray; only Quit exits.
            crate::window::install_close_handler(&app.handle().clone());

            // First-frame visibility (spec 03 §12): a NORMAL launch shows +
            // focuses the window; an AUTOSTART (login) launch stays hidden in the
            // tray. The window boots hidden (`visible: false`), so we only ever
            // opt IN to showing — an autostart launch simply leaves it hidden.
            if launched_from_autostart() {
                tracing::info!("launched from autostart; staying hidden in the tray");
            } else {
                crate::window::show_window(&app.handle().clone());
            }

            Ok(())
        })
        // `expect` is acceptable at this binary edge: a failure here means the
        // app cannot start at all (AGENTS.md §4 — provably-terminal, bin edge).
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
