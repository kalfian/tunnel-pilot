//! System tray icon + interaction (spec 03 §§10,11; 02 §8).
//!
//! `setup` builds the tray with the dynamic count icon and the rich, state-driven
//! **native** menu (Laravel Herd / Tailscale style). The menu is shown on BOTH
//! left- and right-click — standard macOS menu-bar behaviour — so the OS
//! auto-positions it directly under the icon (no manual anchoring, no
//! mis-positioning). Clicking the tray icon simply opens the native `NSMenu`.
//!
//! The dynamic count icon (idle grey / 1–9 badge) and the menu itself are kept in
//! sync with `tunnel://status` + `update://status` by [`menu::spawn_tray_sync`].

pub mod icon;
pub mod menu;

use std::sync::Arc;

use tauri::tray::TrayIconBuilder;
use tauri::App;

use crate::state::AppState;

/// The single tray icon id, referenced when updating the icon later.
pub const TRAY_ID: &str = "main";

/// Build the tray and start the debounced icon+menu sync (spec 03 §§10,11).
/// Called once from `lib.rs` setup, on the main thread, after `AppState` and
/// `UpdaterState` are managed.
pub fn setup(app: &App, state: Arc<AppState>) -> tauri::Result<()> {
    let handle = app.handle().clone();

    // Initial icon: idle (0 connected) at boot. `spawn_tray_sync` immediately
    // repaints from real state, so this is just the first frame.
    let idle_icon = icon::load_image(icon::TrayIcon::Idle)?;

    // The rich, state-driven native menu, built from current state. Shown on both
    // left- and right-click so the tray behaves like a native menu-bar menu.
    let menu = menu::build_current_menu(&handle, &state)?;

    #[allow(unused_mut)]
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Tunnel Pilot")
        .icon(idle_icon)
        .menu(&menu)
        // Native behaviour: left-click also opens the menu (Tailscale/Herd),
        // which the OS auto-positions under the icon.
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            menu::handle_menu_event(app, event.id.as_ref());
        });

    #[cfg(target_os = "macos")]
    {
        tray = tray.icon_as_template(true);
    }

    tray.build(app)?;

    // Debounced icon + menu refresh on `tunnel://status` / `update://status` +
    // immediate first paint.
    menu::spawn_tray_sync(handle, state);

    Ok(())
}
