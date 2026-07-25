//! The tray popover window (`tray_popover`) — a borderless, transparent,
//! always-on-top panel anchored just below the tray icon, shown on tray
//! LEFT-click (spec 03 §§10,11; tray-popover rework).
//!
//! It is a SEPARATE webview window that loads the SAME `index.html` as the main
//! window; the frontend branches on
//! `getCurrentWebviewWindow().label === 'tray_popover'` to render the compact
//! panel instead of the full settings UI. There is no separate HTML entry.
//!
//! Lifecycle:
//! - Created once at boot ([`create_popover`], from the `setup` hook, hidden).
//! - Tray LEFT-click toggles it ([`toggle_popover`]): anchor → show → focus, or
//!   hide if already visible.
//! - Blur-to-dismiss: losing key/focus hides it, guarded against the transient
//!   blur that fires during the opening click ([`PopoverState`]).
//! - Opening emits [`crate::events::POPOVER_OPENED`] to the popover so its UI
//!   rehydrates fresh state every open.
//!
//! On macOS it is turned into a non-activating panel via a minimal objc2 shim
//! ([`crate::platform::macos::make_nonactivating_popover`]) so clicking it never
//! fronts the app and it stays out of Mission Control / the app-switcher.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::events;

/// The tray popover window label. The frontend branches on this exact string.
pub const POPOVER_LABEL: &str = "tray_popover";

/// Compact popover size (contract with the FE agent): ~320 wide × up to ~460
/// tall. Not resizable.
const POPOVER_WIDTH: f64 = 320.0;
const POPOVER_HEIGHT: f64 = 460.0;

/// How long after a show we ignore a `Focused(false)` on the popover. Showing +
/// focusing the panel can emit a transient blur during the opening click; within
/// this window we treat blur as spurious rather than a dismiss (spec: blur-to-
/// dismiss must not fight the opening click).
const OPEN_GUARD: Duration = Duration::from_millis(300);

/// Guards blur-to-dismiss against the opening-click's transient blur. Managed as
/// `Arc<PopoverState>`; [`show_popover`] stamps the show time and the blur
/// handler checks [`within_open_guard`](PopoverState::within_open_guard).
pub struct PopoverState {
    /// Instant of the most recent show; `None` before the first open.
    last_shown: Mutex<Option<Instant>>,
}

impl PopoverState {
    pub fn new() -> Self {
        Self {
            last_shown: Mutex::new(None),
        }
    }

    /// Record that the popover was just shown (start of the blur-guard window).
    fn mark_shown(&self) {
        // Brief std Mutex, no `.await` held across it.
        if let Ok(mut g) = self.last_shown.lock() {
            *g = Some(Instant::now());
        }
    }

    /// True iff a show happened within [`OPEN_GUARD`] — a `Focused(false)` this
    /// soon after opening is the opening-click transient, not a real dismiss.
    fn within_open_guard(&self) -> bool {
        self.last_shown
            .lock()
            .ok()
            .and_then(|g| *g)
            .map(|t| t.elapsed() < OPEN_GUARD)
            .unwrap_or(false)
    }
}

impl Default for PopoverState {
    fn default() -> Self {
        Self::new()
    }
}

/// Create the hidden `tray_popover` window at boot (called from `setup`, main
/// thread). Borderless / transparent / always-on-top / skip-taskbar / not
/// resizable / hidden / unfocused; loads the default app URL (same `index.html`).
/// Installs blur-to-dismiss + hide-on-close and, on macOS, the non-activating
/// panel shim. `state` is the managed [`PopoverState`], captured by the blur
/// handler.
pub fn create_popover(app: &AppHandle, state: Arc<PopoverState>) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, POPOVER_LABEL, WebviewUrl::default())
        .title("Tunnel Pilot")
        .inner_size(POPOVER_WIDTH, POPOVER_HEIGHT)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .visible(false)
        .focused(false)
        .build()?;

    // macOS: make it a non-activating popover (does not activate the app / stays
    // out of Mission Control + the app-switcher). Best-effort objc2 shim; `setup`
    // runs on the main thread, so AppKit mutation here is safe. `ns_window()`
    // yields a non-`Send` raw pointer, hence the direct (non-dispatched) call.
    #[cfg(target_os = "macos")]
    {
        match window.ns_window() {
            Ok(ns_window) => crate::platform::macos::make_nonactivating_popover(ns_window),
            Err(e) => {
                tracing::warn!(error = %e, "popover ns_window unavailable; nonactivating shim skipped")
            }
        }
    }

    // Blur-to-dismiss + hide-on-close. Losing key/focus hides the popover unless
    // we are still inside the opening-click guard window; the OS close path
    // (e.g. ⌘W while focused) hides rather than destroys so the window persists.
    let handle = app.clone();
    let guard = state;
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => {
            if !guard.within_open_guard() {
                hide_popover(&handle);
            }
        }
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_popover(&handle);
        }
        _ => {}
    });

    Ok(())
}

/// Toggle the popover on tray LEFT-click: hide if visible, otherwise anchor below
/// the tray icon and show + focus it.
pub fn toggle_popover(app: &AppHandle) {
    let Some(window) = app.get_webview_window(POPOVER_LABEL) else {
        tracing::warn!("tray popover window not found; cannot toggle");
        return;
    };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        show_popover(app, &window);
    }
}

/// Anchor the popover just below the tray icon, show + focus it, and tell its UI
/// to rehydrate. Uses `tauri-plugin-positioner`'s `TrayBottomCenter`, which reads
/// the tray-icon rect cached by `on_tray_event` in the tray click handler.
fn show_popover(app: &AppHandle, window: &WebviewWindow) {
    use tauri_plugin_positioner::{Position, WindowExt};

    if let Err(e) = window.move_window(Position::TrayBottomCenter) {
        tracing::warn!(error = %e, "failed to anchor popover to tray; showing at last position");
    }

    // Stamp the show time BEFORE showing so the opening-click blur is guarded.
    if let Some(state) = app.try_state::<Arc<PopoverState>>() {
        state.mark_shown();
    }

    let _ = window.show();
    let _ = window.set_focus();

    // Fresh state on every open — targeted to the popover so the main window does
    // not needlessly rehydrate.
    if let Err(e) = app.emit_to(POPOVER_LABEL, events::POPOVER_OPENED, ()) {
        tracing::warn!(error = %e, "failed to emit popover-opened event");
    }
}

/// Hide the popover (blur-to-dismiss, the `hide_tray_popover` command, or when
/// the main Settings window is shown). No-op if the window is absent.
pub fn hide_popover(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
        let _ = window.hide();
    }
}
