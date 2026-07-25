//! System tray icon + interaction (spec 03 §§10,11; 02 §8; tray-popover rework).
//!
//! `setup` builds the tray with the dynamic count icon and wires two gestures:
//!
//! - **LEFT-click** → toggle the rich [tray popover](crate::window::popover)
//!   (`tray_popover` webview window), anchored below the tray icon via
//!   `tauri-plugin-positioner`.
//! - **RIGHT-click** → a MINIMAL native safety-net menu (Settings + Quit,
//!   [`menu::build_minimal_menu`]) so the app is always usable/quittable even if
//!   the popover fails to load.
//!
//! The dynamic count icon (idle grey / 1–9 badge) is kept in sync with
//! `tunnel://status` by [`menu::spawn_icon_sync`]; the menu itself is static.

pub mod icon;
pub mod menu;

use std::sync::Arc;

use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::App;

use crate::state::AppState;

/// The single tray icon id, referenced when updating the icon later.
pub const TRAY_ID: &str = "main";

/// Build the tray and start the icon sync (spec 03 §§10,11). Called once from
/// `lib.rs` setup, on the main thread, after `AppState` is managed and the
/// `tray_popover` window exists.
pub fn setup(app: &App, state: Arc<AppState>) -> tauri::Result<()> {
    let handle = app.handle().clone();

    // Initial icon: idle (0 connected) at boot. `spawn_icon_sync` immediately
    // repaints from real state, so this is just the first frame.
    let idle_icon = icon::load_image(icon::TrayIcon::Idle)?;

    // Minimal right-click safety-net menu (Settings + Quit). The rich, state-
    // driven menu now lives in the LEFT-click popover.
    let menu = menu::build_minimal_menu(&handle)?;

    #[allow(unused_mut)]
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Tunnel Pilot")
        .icon(idle_icon)
        .menu(&menu)
        // LEFT-click must reach `on_tray_icon_event` (toggle the popover) rather
        // than opening the native menu; the menu shows on RIGHT-click.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            menu::handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();
            // Feed the tray-icon rect to the positioner so `TrayBottomCenter`
            // can anchor the popover below the icon.
            tauri_plugin_positioner::on_tray_event(app, &event);
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::window::popover::toggle_popover(app);
            }
        });

    #[cfg(target_os = "macos")]
    {
        tray = tray.icon_as_template(true);
    }

    tray.build(app)?;

    // Debounced count-icon refresh on `tunnel://status` + immediate first paint.
    menu::spawn_icon_sync(handle, state);

    Ok(())
}
