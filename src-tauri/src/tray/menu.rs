//! Tray menu build + debounced rebuild-on-change: per-tunnel rows (Retry on
//! error), conditional bulk Start/Stop All, update-notice slot (spec 03 §10/§11).
//!
//! The menu *model* ([`build_menu_model`]) is a pure function of the tunnel
//! states + update availability, so the "which rows/actions/bulk items appear"
//! logic is unit-testable without a display. Turning the model into a real
//! `tauri::menu::Menu` and reacting to clicks is the impure layer below.
//!
//! Rebuilds are **debounced** (~100 ms): a burst of `tunnel://status` events
//! (e.g. Start All flipping every tunnel) coalesces into a single rebuild
//! instead of thrashing the menu once per event (spec 03 §10).

use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Listener, Manager, Wry};
use tokio::sync::Notify;

use crate::events;
use crate::state::models::ForwardStatus;
use crate::state::AppState;

/// Debounce window for coalescing rapid status changes into one rebuild.
const REBUILD_DEBOUNCE: Duration = Duration::from_millis(100);

// --- menu item ids ---------------------------------------------------------

pub const ID_OPEN: &str = "open";
pub const ID_QUIT: &str = "quit";
pub const ID_START_ALL: &str = "start_all";
pub const ID_STOP_ALL: &str = "stop_all";
pub const ID_UPDATE: &str = "update_install";

/// Per-tunnel item ids are `"t:<action>:<uuid>"`. UUIDs contain no `:` so a
/// `splitn(3, ':')` cleanly recovers `(action, id)`.
const TUNNEL_PREFIX: &str = "t";

// --- pure model ------------------------------------------------------------

/// A tunnel's identity + live status, the input to the pure menu model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelState {
    pub id: String,
    pub name: String,
    pub status: ForwardStatus,
}

/// The action a per-tunnel menu row can offer, given its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelAction {
    Connect,
    Disconnect,
    Retry,
}

/// One per-tunnel row in the tray menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelMenuRow {
    pub id: String,
    pub name: String,
    pub status: ForwardStatus,
    /// Actions available in this status (empty for the transient `disconnecting`).
    pub actions: Vec<TunnelAction>,
}

/// The full tray-menu model — a pure function of state (see [`build_menu_model`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// Update-notice slot at the top; always `false` until M6 wires the updater.
    pub update_available: bool,
    pub tunnels: Vec<TunnelMenuRow>,
    /// "Start All" shown when at least one tunnel is startable.
    pub show_start_all: bool,
    /// "Stop All" shown when at least one tunnel is stoppable.
    pub show_stop_all: bool,
}

/// The actions offered for a given status (spec 03 §11). `disconnecting` is a
/// transient — clicks are ignored, so it offers nothing (F23).
fn actions_for(status: ForwardStatus) -> Vec<TunnelAction> {
    match status {
        ForwardStatus::Disconnected => vec![TunnelAction::Connect],
        ForwardStatus::Connecting => vec![TunnelAction::Disconnect],
        ForwardStatus::Connected => vec![TunnelAction::Disconnect],
        ForwardStatus::Disconnecting => vec![],
        // `error` always exposes Retry (acceptance), plus Disconnect to give up.
        ForwardStatus::Error => vec![TunnelAction::Retry, TunnelAction::Disconnect],
    }
}

/// Build the pure tray-menu model from the current tunnel states + update
/// availability. Bulk items appear conditionally: Start All when anything is
/// startable (disconnected/error), Stop All when anything is stoppable
/// (connected/connecting).
pub fn build_menu_model(tunnels: &[TunnelState], update_available: bool) -> MenuModel {
    let rows: Vec<TunnelMenuRow> = tunnels
        .iter()
        .map(|t| TunnelMenuRow {
            id: t.id.clone(),
            name: t.name.clone(),
            status: t.status,
            actions: actions_for(t.status),
        })
        .collect();

    let show_start_all = tunnels
        .iter()
        .any(|t| matches!(t.status, ForwardStatus::Disconnected | ForwardStatus::Error));
    let show_stop_all = tunnels.iter().any(|t| {
        matches!(
            t.status,
            ForwardStatus::Connected | ForwardStatus::Connecting
        )
    });

    MenuModel {
        update_available,
        tunnels: rows,
        show_start_all,
        show_stop_all,
    }
}

/// Number of tunnels in `Connected` — drives the tray badge count.
pub fn connected_count(tunnels: &[TunnelState]) -> usize {
    tunnels
        .iter()
        .filter(|t| t.status == ForwardStatus::Connected)
        .count()
}

/// A short status glyph + label for menu display (geometric symbols, not emoji).
fn status_glyph(status: ForwardStatus) -> &'static str {
    match status {
        ForwardStatus::Connected => "●",
        ForwardStatus::Connecting => "◐",
        ForwardStatus::Disconnected => "○",
        ForwardStatus::Disconnecting => "◌",
        ForwardStatus::Error => "⚠",
    }
}

fn status_label(status: ForwardStatus) -> &'static str {
    match status {
        ForwardStatus::Connected => "Connected",
        ForwardStatus::Connecting => "Connecting…",
        ForwardStatus::Disconnected => "Disconnected",
        ForwardStatus::Disconnecting => "Disconnecting…",
        ForwardStatus::Error => "Error",
    }
}

// --- state gathering -------------------------------------------------------

/// Snapshot the configured forwards + their live status into [`TunnelState`]s,
/// preserving display (config) order. A config with no live supervisor reads as
/// `Disconnected`.
pub fn gather_tunnel_states(state: &AppState) -> Vec<TunnelState> {
    state
        .configs_snapshot()
        .into_iter()
        .map(|c| {
            let status = state
                .registry
                .current_status(&c.id)
                .unwrap_or(ForwardStatus::Disconnected);
            TunnelState {
                id: c.id,
                name: c.name,
                status,
            }
        })
        .collect()
}

// --- tauri menu construction ----------------------------------------------

/// Build a concrete `tauri::menu::Menu` from the pure model. Must run on the
/// main thread (AppKit) — callers dispatch via `run_on_main_thread`.
pub fn build_tauri_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Menu<Wry>> {
    // Boxed so heterogeneous item kinds (MenuItem/Submenu/separator) share a Vec.
    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();

    // Update-notice slot (top). Empty until M6 sets `update_available`.
    if model.update_available {
        items.push(Box::new(MenuItem::with_id(
            app,
            ID_UPDATE,
            "Install update…",
            true,
            None::<&str>,
        )?));
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
    }

    // Per-tunnel rows.
    if model.tunnels.is_empty() {
        items.push(Box::new(MenuItem::with_id(
            app,
            "noop:empty",
            "No forwards configured",
            false,
            None::<&str>,
        )?));
    } else {
        for row in &model.tunnels {
            let label = format!("{}  {}", status_glyph(row.status), row.name);
            if row.actions.is_empty() {
                // Transient (disconnecting) — a disabled label, no actions.
                items.push(Box::new(MenuItem::with_id(
                    app,
                    format!("noop:{}", row.id),
                    format!("{}  ({})", label, status_label(row.status)),
                    false,
                    None::<&str>,
                )?));
            } else {
                items.push(Box::new(build_tunnel_submenu(app, row, &label)?));
            }
        }
    }

    items.push(Box::new(PredefinedMenuItem::separator(app)?));

    // Conditional global bulk actions.
    if model.show_start_all {
        items.push(Box::new(MenuItem::with_id(
            app,
            ID_START_ALL,
            "Start All",
            true,
            None::<&str>,
        )?));
    }
    if model.show_stop_all {
        items.push(Box::new(MenuItem::with_id(
            app,
            ID_STOP_ALL,
            "Stop All",
            true,
            None::<&str>,
        )?));
    }
    if model.show_start_all || model.show_stop_all {
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
    }

    items.push(Box::new(MenuItem::with_id(
        app,
        ID_OPEN,
        "Open",
        true,
        None::<&str>,
    )?));
    items.push(Box::new(MenuItem::with_id(
        app,
        ID_QUIT,
        "Quit",
        true,
        None::<&str>,
    )?));

    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(app, &refs)
}

/// Build the submenu for a single tunnel row: a disabled status line + the
/// available action items.
fn build_tunnel_submenu(
    app: &AppHandle,
    row: &TunnelMenuRow,
    label: &str,
) -> tauri::Result<Submenu<Wry>> {
    let mut children: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();

    children.push(Box::new(MenuItem::with_id(
        app,
        format!("noop:status:{}", row.id),
        format!("Status: {}", status_label(row.status)),
        false,
        None::<&str>,
    )?));
    children.push(Box::new(PredefinedMenuItem::separator(app)?));

    for action in &row.actions {
        let (id, text) = match action {
            TunnelAction::Connect => (tunnel_item_id("connect", &row.id), "Connect"),
            TunnelAction::Disconnect => (tunnel_item_id("disconnect", &row.id), "Disconnect"),
            TunnelAction::Retry => (tunnel_item_id("retry", &row.id), "Retry"),
        };
        children.push(Box::new(MenuItem::with_id(
            app,
            id,
            text,
            true,
            None::<&str>,
        )?));
    }

    let refs: Vec<&dyn IsMenuItem<Wry>> = children.iter().map(|b| b.as_ref()).collect();
    Submenu::with_items(app, label, true, &refs)
}

fn tunnel_item_id(action: &str, id: &str) -> String {
    format!("{TUNNEL_PREFIX}:{action}:{id}")
}

/// Parse a per-tunnel menu id back into `(action, forward_id)`. Returns `None`
/// for non-tunnel ids (open/quit/bulk/noop).
fn parse_tunnel_item_id(item_id: &str) -> Option<(&str, &str)> {
    let mut parts = item_id.splitn(3, ':');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(TUNNEL_PREFIX), Some(action), Some(id)) => Some((action, id)),
        _ => None,
    }
}

// --- click routing ---------------------------------------------------------

/// Route a tray menu click to the right action. Sync (Tauri calls it on the
/// menu-event thread); engine work is spawned onto the async runtime.
pub fn handle_menu_event(app: &AppHandle, item_id: &str) {
    match item_id {
        ID_OPEN => crate::window::show_window(app),
        ID_QUIT => crate::window::quit_app(app),
        ID_START_ALL => spawn_engine(app, |state| async move {
            let _ = crate::commands::forwards::run_start_all(&state).await;
        }),
        ID_STOP_ALL => spawn_engine(app, |state| async move {
            let _ = crate::commands::forwards::run_stop_all(&state).await;
        }),
        ID_UPDATE => {
            // Update-notice slot: the install command lands in M6. Until then the
            // item is only ever present when `update_available` is forced false,
            // so this branch is unreachable in M3 — logged for safety.
            tracing::warn!("tray update-install clicked but updater lands in M6");
        }
        other => {
            if let Some((action, id)) = parse_tunnel_item_id(other) {
                let id = id.to_string();
                match action {
                    "connect" => spawn_engine(app, move |state| async move {
                        if let Err(e) = crate::ssh::engine::connect_forward(&state, &id).await {
                            tracing::error!(error = %e, "tray connect failed");
                        }
                    }),
                    "disconnect" => spawn_engine(app, move |state| async move {
                        // User-initiated from the tray → silent (no notification).
                        if let Err(e) =
                            crate::ssh::engine::disconnect_forward(&state, &id, true).await
                        {
                            tracing::error!(error = %e, "tray disconnect failed");
                        }
                    }),
                    "retry" => spawn_engine(app, move |state| async move {
                        if let Err(e) = crate::ssh::engine::retry_forward(&state, &id).await {
                            tracing::error!(error = %e, "tray retry failed");
                        }
                    }),
                    _ => {}
                }
            }
            // "noop:*" disabled labels never fire an event; ignore anything else.
        }
    }
}

/// Fetch `AppState` from the app and spawn an engine future built from it.
fn spawn_engine<F, Fut>(app: &AppHandle, f: F)
where
    F: FnOnce(Arc<AppState>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let Some(state) = app.try_state::<Arc<AppState>>() else {
        tracing::error!("AppState not managed; tray action dropped");
        return;
    };
    let state = state.inner().clone();
    tauri::async_runtime::spawn(f(state));
}

// --- rebuild + debounce -----------------------------------------------------

/// Rebuild the tray icon + menu from current state, on the main thread.
pub fn rebuild_now(app: &AppHandle, state: &Arc<AppState>) {
    let tunnels = gather_tunnel_states(state);
    let count = connected_count(&tunnels);
    // Update notice is empty until M6.
    let model = build_menu_model(&tunnels, false);

    let app_main = app.clone();
    let dispatch = app.run_on_main_thread(move || {
        super::icon::update_tray_icon(&app_main, super::TRAY_ID, count);
        match build_tauri_menu(&app_main, &model) {
            Ok(menu) => {
                if let Some(tray) = app_main.tray_by_id(super::TRAY_ID) {
                    if let Err(e) = tray.set_menu(Some(menu)) {
                        tracing::error!(error = %e, "failed to set tray menu");
                    }
                } else {
                    tracing::warn!("tray not found; cannot set menu");
                }
            }
            Err(e) => tracing::error!(error = %e, "failed to build tray menu"),
        }
    });
    if let Err(e) = dispatch {
        tracing::error!(error = %e, "failed to dispatch tray rebuild to main thread");
    }
}

/// Subscribe to `tunnel://status` and rebuild the tray (icon + menu) on change,
/// debounced ~100 ms so a bulk operation coalesces into one rebuild.
pub fn spawn_tray_sync(app: AppHandle, state: Arc<AppState>) {
    let notify = Arc::new(Notify::new());

    // Any status transition marks the tray dirty.
    let dirty = notify.clone();
    app.listen(events::TUNNEL_STATUS, move |_event| {
        dirty.notify_one();
    });

    // Debounce loop: wake on the first change, wait out the window (coalescing
    // further changes), then do exactly one rebuild.
    let debounce_app = app.clone();
    let debounce_state = state.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            notify.notified().await;
            tokio::time::sleep(REBUILD_DEBOUNCE).await;
            rebuild_now(&debounce_app, &debounce_state);
        }
    });

    // Initial build so the menu reflects the boot state immediately.
    rebuild_now(&app, &state);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(id: &str, status: ForwardStatus) -> TunnelState {
        TunnelState {
            id: id.to_string(),
            name: format!("fwd-{id}"),
            status,
        }
    }

    #[test]
    fn empty_has_no_bulk() {
        let m = build_menu_model(&[], false);
        assert!(m.tunnels.is_empty());
        assert!(!m.show_start_all);
        assert!(!m.show_stop_all);
        assert!(!m.update_available);
    }

    #[test]
    fn error_row_exposes_retry() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Error)], false);
        let row = &m.tunnels[0];
        assert!(row.actions.contains(&TunnelAction::Retry));
    }

    #[test]
    fn connected_row_offers_disconnect_only() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Connected)], false);
        assert_eq!(m.tunnels[0].actions, vec![TunnelAction::Disconnect]);
    }

    #[test]
    fn disconnected_row_offers_connect_only() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Disconnected)], false);
        assert_eq!(m.tunnels[0].actions, vec![TunnelAction::Connect]);
    }

    #[test]
    fn disconnecting_row_has_no_actions() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Disconnecting)], false);
        assert!(m.tunnels[0].actions.is_empty());
    }

    #[test]
    fn start_all_shown_when_something_startable() {
        // A disconnected tunnel is startable; a connected one is not.
        let m = build_menu_model(
            &[
                ts("a", ForwardStatus::Connected),
                ts("b", ForwardStatus::Disconnected),
            ],
            false,
        );
        assert!(m.show_start_all);
        assert!(m.show_stop_all);
    }

    #[test]
    fn start_all_hidden_when_all_connected() {
        let m = build_menu_model(
            &[
                ts("a", ForwardStatus::Connected),
                ts("b", ForwardStatus::Connected),
            ],
            false,
        );
        assert!(!m.show_start_all);
        assert!(m.show_stop_all);
    }

    #[test]
    fn stop_all_hidden_when_all_disconnected() {
        let m = build_menu_model(
            &[
                ts("a", ForwardStatus::Disconnected),
                ts("b", ForwardStatus::Error),
            ],
            false,
        );
        assert!(m.show_start_all);
        assert!(!m.show_stop_all);
    }

    #[test]
    fn error_makes_start_all_available() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Error)], false);
        assert!(m.show_start_all);
        assert!(!m.show_stop_all);
    }

    #[test]
    fn connecting_makes_stop_all_available() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Connecting)], false);
        assert!(!m.show_start_all);
        assert!(m.show_stop_all);
    }

    #[test]
    fn connected_count_counts_only_connected() {
        let tunnels = [
            ts("a", ForwardStatus::Connected),
            ts("b", ForwardStatus::Connected),
            ts("c", ForwardStatus::Connecting),
            ts("d", ForwardStatus::Error),
        ];
        assert_eq!(connected_count(&tunnels), 2);
    }

    #[test]
    fn update_slot_flows_through() {
        let m = build_menu_model(&[], true);
        assert!(m.update_available);
    }

    #[test]
    fn tunnel_item_id_roundtrips() {
        let id = "550e8400-e29b-41d4-a716-446655440000";
        let item = tunnel_item_id("retry", id);
        assert_eq!(parse_tunnel_item_id(&item), Some(("retry", id)));
    }

    #[test]
    fn non_tunnel_ids_do_not_parse() {
        assert_eq!(parse_tunnel_item_id(ID_OPEN), None);
        assert_eq!(parse_tunnel_item_id(ID_START_ALL), None);
        assert_eq!(parse_tunnel_item_id("noop:status:abc"), None);
    }
}
