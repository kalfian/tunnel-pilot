//! System tray icon + menu (spec 03 §§10,11; 02 §8).
//!
//! `setup` builds the tray with the dynamic count icon + the initial menu, wires
//! menu clicks to engine/window actions, and starts the debounced sync task that
//! rebuilds icon + menu on `tunnel://status` changes.

pub mod icon;
pub mod menu;

use std::sync::Arc;

use tauri::tray::TrayIconBuilder;
use tauri::App;

use crate::state::AppState;

/// The single tray icon id, referenced when updating icon/menu later.
pub const TRAY_ID: &str = "main";

/// Build the tray and start the state-driven sync (spec 03 §§10,11). Called once
/// from `lib.rs` setup, on the main thread, after `AppState` is managed.
pub fn setup(app: &App, state: Arc<AppState>) -> tauri::Result<()> {
    let handle = app.handle().clone();

    // Initial icon: idle (0 connected) at boot. `spawn_tray_sync` immediately
    // rebuilds from real state, so this is just the first frame.
    let idle_icon = icon::load_image(icon::TrayIcon::Idle)?;

    // Initial menu from current state so the tray is correct the instant it
    // appears (before the first sync tick). `spawn_tray_sync` immediately
    // rebuilds (reading the cached update status too), so at boot the notice is
    // absent unless the auto-check already found an update.
    let tunnels = menu::gather_tunnel_states(&state);
    let model = menu::build_menu_model(&tunnels, None);
    let initial_menu = menu::build_tauri_menu(&handle, &model)?;

    #[allow(unused_mut)]
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Tunnel Pilot")
        .icon(idle_icon)
        .menu(&initial_menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            menu::handle_menu_event(app, event.id.as_ref());
        });

    #[cfg(target_os = "macos")]
    {
        tray = tray.icon_as_template(true);
    }

    tray.build(app)?;

    // Debounced rebuild-on-change + immediate correct-state rebuild.
    menu::spawn_tray_sync(handle, state);

    Ok(())
}
