//! Tray menu build + debounced rebuild-on-change, styled after Laravel Herd's
//! native menu-bar menu: an update-notice slot at the top, per-tunnel rows with
//! a colored status-dot leading icon, conditional bulk Start/Stop All, and an
//! actions footer (Settings ⌘, / Check for Updates… ⌘U / Quit ⌘Q).
//!
//! The menu *model* ([`build_menu_model`]) is a pure function of the tunnel
//! states + update availability, so the "which rows/actions/bulk items appear"
//! logic is unit-testable without a display. Turning the model into a real
//! `tauri::menu::Menu` (with `IconMenuItem` dots + accelerators) and reacting to
//! clicks is the impure layer below.
//!
//! Each per-tunnel row is a single clickable `IconMenuItem`: clicking performs
//! the row's *primary* action for its status — connect when disconnected,
//! disconnect when connected/connecting, **retry** when errored. The transient
//! `disconnecting` row is disabled (clicks ignored).
//!
//! Rebuilds are **debounced** (~100 ms): a burst of `tunnel://status` events
//! (e.g. Start All flipping every tunnel) coalesces into a single rebuild
//! instead of thrashing the menu once per event (spec 03 §10).

use std::sync::Arc;
use std::time::Duration;

use tauri::menu::{IconMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Listener, Manager, Wry};
use tokio::sync::Notify;

use crate::events;
use crate::state::models::{ForwardStatus, UpdateStatus};
use crate::state::AppState;
use crate::updater::UpdaterState;

/// Debounce window for coalescing rapid status changes into one rebuild.
const REBUILD_DEBOUNCE: Duration = Duration::from_millis(100);

// --- menu item ids ---------------------------------------------------------

pub const ID_OPEN: &str = "open";
pub const ID_QUIT: &str = "quit";
pub const ID_START_ALL: &str = "start_all";
pub const ID_STOP_ALL: &str = "stop_all";
pub const ID_UPDATE: &str = "update_install";
pub const ID_CHECK_UPDATE: &str = "check_update";

// --- accelerators (rendered right-aligned, Herd-style) ---------------------

/// Settings — ⌘, (macOS) / Ctrl+, elsewhere.
const ACCEL_SETTINGS: &str = "CmdOrCtrl+,";
/// Check for Updates… — ⌘U / Ctrl+U.
const ACCEL_CHECK_UPDATE: &str = "CmdOrCtrl+U";
/// Quit — ⌘Q / Ctrl+Q.
const ACCEL_QUIT: &str = "CmdOrCtrl+Q";

/// Per-tunnel item ids are `"t:<action>:<uuid>"`. UUIDs contain no `:` so a
/// `splitn(3, ':')` cleanly recovers `(action, id)`.
const TUNNEL_PREFIX: &str = "t";

// --- pure model ------------------------------------------------------------

/// A tunnel's identity + live status, the input to the pure menu model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TunnelState {
    pub id: String,
    pub name: String,
    /// Local bind port, shown right of the name (e.g. `Qismo Prod   :5431`).
    pub port: u16,
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
    pub port: u16,
    pub status: ForwardStatus,
    /// Actions available in this status (empty for the transient `disconnecting`).
    pub actions: Vec<TunnelAction>,
    /// The single action a click on this row performs (`None` while transient).
    pub primary: Option<TunnelAction>,
}

/// Content of the update-notice slot when an update is pending install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateNotice {
    /// The available version (e.g. `"1.5.0"`), if the updater reported one.
    pub version: Option<String>,
}

/// The full tray-menu model — a pure function of state (see [`build_menu_model`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuModel {
    /// Update-notice slot at the top; `Some` only when an update is pending
    /// install (available and not skipped) — see [`update_notice_from_status`].
    pub update_notice: Option<UpdateNotice>,
    pub tunnels: Vec<TunnelMenuRow>,
    /// "Start All" shown when at least one tunnel is startable.
    pub show_start_all: bool,
    /// "Stop All" shown when at least one tunnel is stoppable.
    pub show_stop_all: bool,
}

/// Derive the tray update-notice from the cached [`UpdateStatus`]. An update is
/// offered in the tray only when it is available **and not skipped** — a version
/// the user dismissed (`lastSkippedVersion`) must not be re-offered here.
pub fn update_notice_from_status(status: &UpdateStatus) -> Option<UpdateNotice> {
    if status.available && !status.skipped {
        Some(UpdateNotice {
            version: status.version.clone(),
        })
    } else {
        None
    }
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

/// The single action a click on a per-tunnel row performs (Herd-style flat
/// rows). Connect when idle, disconnect when live, **retry** when errored;
/// `None` while `disconnecting` (row disabled, clicks ignored — F23).
pub fn primary_action(status: ForwardStatus) -> Option<TunnelAction> {
    match status {
        ForwardStatus::Disconnected => Some(TunnelAction::Connect),
        ForwardStatus::Connecting | ForwardStatus::Connected => Some(TunnelAction::Disconnect),
        ForwardStatus::Error => Some(TunnelAction::Retry),
        ForwardStatus::Disconnecting => None,
    }
}

/// Build the pure tray-menu model from the current tunnel states + update
/// availability. Bulk items appear conditionally: Start All when anything is
/// startable (disconnected/error), Stop All when anything is stoppable
/// (connected/connecting).
pub fn build_menu_model(tunnels: &[TunnelState], update_notice: Option<UpdateNotice>) -> MenuModel {
    let rows: Vec<TunnelMenuRow> = tunnels
        .iter()
        .map(|t| TunnelMenuRow {
            id: t.id.clone(),
            name: t.name.clone(),
            port: t.port,
            status: t.status,
            actions: actions_for(t.status),
            primary: primary_action(t.status),
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
        update_notice,
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

fn status_label(status: ForwardStatus) -> &'static str {
    match status {
        ForwardStatus::Connected => "Connected",
        ForwardStatus::Connecting => "Connecting…",
        ForwardStatus::Disconnected => "Disconnected",
        ForwardStatus::Disconnecting => "Disconnecting…",
        ForwardStatus::Error => "Error",
    }
}

/// The label for a per-tunnel row: name with the local port appended, Herd-style
/// (`Qismo Prod   :5431`). A transient `disconnecting` row also carries its
/// state so the disabled row reads clearly.
fn tunnel_row_label(row: &TunnelMenuRow) -> String {
    let base = format!("{}   :{}", row.name, row.port);
    if row.primary.is_none() {
        format!("{base}  ({})", status_label(row.status))
    } else {
        base
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
                port: c.local_port,
                status,
            }
        })
        .collect()
}

// --- tauri menu construction ----------------------------------------------

/// Build the rich, state-driven native menu from current app + updater state.
/// This is the menu shown on tray click; [`spawn_tray_sync`] rebuilds it on
/// every status/update change. Must run on the main thread (AppKit).
pub fn build_current_menu(app: &AppHandle, state: &Arc<AppState>) -> tauri::Result<Menu<Wry>> {
    let tunnels = gather_tunnel_states(state);
    let update_notice = gather_update_notice(app);
    let model = build_menu_model(&tunnels, update_notice);
    build_tauri_menu(app, &model)
}

/// Build a concrete `tauri::menu::Menu` from the pure model. Must run on the
/// main thread (AppKit) — callers dispatch via `run_on_main_thread`.
pub fn build_tauri_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Menu<Wry>> {
    // Boxed so heterogeneous item kinds (MenuItem/Submenu/separator) share a Vec.
    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();

    // Update-notice slot (top). Present only when an update is pending install.
    if let Some(notice) = &model.update_notice {
        let label = match &notice.version {
            Some(v) => format!("Update available (v{v}) — Install"),
            None => "Update available — Install".to_string(),
        };
        items.push(Box::new(MenuItem::with_id(
            app,
            ID_UPDATE,
            label,
            true,
            None::<&str>,
        )?));
        items.push(Box::new(PredefinedMenuItem::separator(app)?));
    }

    // Per-tunnel rows: each a single clickable IconMenuItem with a colored
    // status-dot leading icon. Clicking runs the row's primary action.
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
            items.push(build_tunnel_row(app, row)?);
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

    // Actions footer with accelerators (rendered right-aligned, Herd-style).
    //
    // The main window IS the settings/config window (v1 parity), so the tray
    // entry reads "Settings". Id stays `ID_OPEN` — the action still shows the
    // window via `window::show_window` (see `handle_menu_event`).
    items.push(Box::new(MenuItem::with_id(
        app,
        ID_OPEN,
        "Settings",
        true,
        Some(ACCEL_SETTINGS),
    )?));
    items.push(Box::new(MenuItem::with_id(
        app,
        ID_CHECK_UPDATE,
        "Check for Updates…",
        true,
        Some(ACCEL_CHECK_UPDATE),
    )?));
    items.push(Box::new(MenuItem::with_id(
        app,
        ID_QUIT,
        "Quit",
        true,
        Some(ACCEL_QUIT),
    )?));

    let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|b| b.as_ref()).collect();
    Menu::with_items(app, &refs)
}

/// Build a single per-tunnel row as an `IconMenuItem` (colored status dot +
/// name/port). Enabled rows carry the primary-action id (`t:<action>:<uuid>`);
/// a transient `disconnecting` row is disabled with a `noop:` id. If the dot
/// image fails to decode, the row still renders (icon omitted).
fn build_tunnel_row(
    app: &AppHandle,
    row: &TunnelMenuRow,
) -> tauri::Result<Box<dyn IsMenuItem<Wry>>> {
    let label = tunnel_row_label(row);
    let dot = super::icon::load_dot(row.status).ok();

    let (id, enabled) = match row.primary {
        Some(TunnelAction::Connect) => (tunnel_item_id("connect", &row.id), true),
        Some(TunnelAction::Disconnect) => (tunnel_item_id("disconnect", &row.id), true),
        Some(TunnelAction::Retry) => (tunnel_item_id("retry", &row.id), true),
        None => (format!("noop:{}", row.id), false),
    };

    Ok(Box::new(IconMenuItem::with_id(
        app,
        id,
        label,
        enabled,
        dot,
        None::<&str>,
    )?))
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
            // Install the pending update via the same path as the `install_update`
            // command (updater::run_install downloads → verifies minisign → installs
            // → relaunches). Spawned onto the async runtime; errors are logged.
            let Some(updater) = app.try_state::<Arc<UpdaterState>>() else {
                tracing::error!("UpdaterState not managed; tray update-install dropped");
                return;
            };
            let updater = updater.inner().clone();
            let app_for_install = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::updater::run_install(&app_for_install, &updater).await {
                    tracing::error!(error = %e, "tray update-install failed");
                }
            });
        }
        ID_CHECK_UPDATE => {
            // Manual "Check for Updates…" — same path as the `check_update`
            // command (no auto-notify: the user asked, so any result surfaces via
            // the emitted `update://status` → rebuilds the tray notice slot).
            let (Some(state), Some(updater)) = (
                app.try_state::<Arc<AppState>>(),
                app.try_state::<Arc<UpdaterState>>(),
            ) else {
                tracing::error!("state/updater not managed; tray update-check dropped");
                return;
            };
            let state = state.inner().clone();
            let updater = updater.inner().clone();
            let app_for_check = app.clone();
            tauri::async_runtime::spawn(async move {
                // User-initiated (tray) → a failure surfaces as a clean string in
                // the emitted `update://status`, not an error object (BUG 3).
                if let Err(e) = crate::updater::run_check(
                    &app_for_check,
                    &state,
                    &updater,
                    crate::updater::CheckTrigger::UserRequested,
                )
                .await
                {
                    tracing::error!(error = %e, "tray update-check failed");
                }
            });
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

/// Read the cached update availability from the managed [`UpdaterState`] into a
/// tray [`UpdateNotice`]. Returns `None` if the updater state isn't managed yet
/// or no update is pending. The `latest_status` lock is a brief std `Mutex` with
/// no `.await` held across it.
fn gather_update_notice(app: &AppHandle) -> Option<UpdateNotice> {
    let updater = app.try_state::<Arc<UpdaterState>>()?;
    let status = updater.latest_status();
    update_notice_from_status(&status)
}

/// Rebuild the tray icon + menu from current state, on the main thread.
pub fn rebuild_now(app: &AppHandle, state: &Arc<AppState>) {
    let tunnels = gather_tunnel_states(state);
    let count = connected_count(&tunnels);
    let update_notice = gather_update_notice(app);
    let model = build_menu_model(&tunnels, update_notice);

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

    // Any tunnel status transition marks the tray dirty.
    let dirty = notify.clone();
    app.listen(events::TUNNEL_STATUS, move |_event| {
        dirty.notify_one();
    });

    // Update-availability changes also rebuild the tray (to show/hide the
    // update-notice slot), coalesced through the same debounce.
    let dirty_update = notify.clone();
    app.listen(events::UPDATE_STATUS, move |_event| {
        dirty_update.notify_one();
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
            port: 5000,
            status,
        }
    }

    #[test]
    fn empty_has_no_bulk() {
        let m = build_menu_model(&[], None);
        assert!(m.tunnels.is_empty());
        assert!(!m.show_start_all);
        assert!(!m.show_stop_all);
        assert!(m.update_notice.is_none());
    }

    #[test]
    fn error_row_exposes_retry() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Error)], None);
        let row = &m.tunnels[0];
        assert!(row.actions.contains(&TunnelAction::Retry));
    }

    #[test]
    fn connected_row_offers_disconnect_only() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Connected)], None);
        assert_eq!(m.tunnels[0].actions, vec![TunnelAction::Disconnect]);
    }

    #[test]
    fn disconnected_row_offers_connect_only() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Disconnected)], None);
        assert_eq!(m.tunnels[0].actions, vec![TunnelAction::Connect]);
    }

    #[test]
    fn disconnecting_row_has_no_actions() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Disconnecting)], None);
        assert!(m.tunnels[0].actions.is_empty());
    }

    #[test]
    fn primary_action_toggles_by_status() {
        // Flat-row click behaviour: idle→connect, live→disconnect, error→retry.
        assert_eq!(
            primary_action(ForwardStatus::Disconnected),
            Some(TunnelAction::Connect)
        );
        assert_eq!(
            primary_action(ForwardStatus::Connecting),
            Some(TunnelAction::Disconnect)
        );
        assert_eq!(
            primary_action(ForwardStatus::Connected),
            Some(TunnelAction::Disconnect)
        );
        assert_eq!(
            primary_action(ForwardStatus::Error),
            Some(TunnelAction::Retry)
        );
        assert_eq!(primary_action(ForwardStatus::Disconnecting), None);
    }

    #[test]
    fn error_row_primary_is_retry() {
        // On error, clicking the row retries (keeps a way to retry — acceptance).
        let m = build_menu_model(&[ts("a", ForwardStatus::Error)], None);
        assert_eq!(m.tunnels[0].primary, Some(TunnelAction::Retry));
    }

    #[test]
    fn disconnecting_row_has_no_primary() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Disconnecting)], None);
        assert!(m.tunnels[0].primary.is_none());
    }

    #[test]
    fn row_carries_port_and_label() {
        let mut t = ts("a", ForwardStatus::Connected);
        t.name = "Qismo Prod".to_string();
        t.port = 5431;
        let m = build_menu_model(&[t], None);
        let row = &m.tunnels[0];
        assert_eq!(row.port, 5431);
        assert_eq!(tunnel_row_label(row), "Qismo Prod   :5431");
    }

    #[test]
    fn disconnecting_label_includes_state() {
        let mut t = ts("a", ForwardStatus::Disconnecting);
        t.name = "DB".to_string();
        t.port = 6001;
        let m = build_menu_model(&[t], None);
        assert_eq!(
            tunnel_row_label(&m.tunnels[0]),
            "DB   :6001  (Disconnecting…)"
        );
    }

    #[test]
    fn start_all_shown_when_something_startable() {
        // A disconnected tunnel is startable; a connected one is not.
        let m = build_menu_model(
            &[
                ts("a", ForwardStatus::Connected),
                ts("b", ForwardStatus::Disconnected),
            ],
            None,
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
            None,
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
            None,
        );
        assert!(m.show_start_all);
        assert!(!m.show_stop_all);
    }

    #[test]
    fn error_makes_start_all_available() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Error)], None);
        assert!(m.show_start_all);
        assert!(!m.show_stop_all);
    }

    #[test]
    fn connecting_makes_stop_all_available() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Connecting)], None);
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
    fn update_notice_flows_through_when_present() {
        let notice = UpdateNotice {
            version: Some("1.5.0".to_string()),
        };
        let m = build_menu_model(&[], Some(notice.clone()));
        assert_eq!(m.update_notice, Some(notice));
    }

    #[test]
    fn update_notice_absent_when_none() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Connected)], None);
        assert!(m.update_notice.is_none());
    }

    #[test]
    fn update_notice_from_status_offered_when_available_not_skipped() {
        let status = UpdateStatus {
            available: true,
            version: Some("1.5.0".to_string()),
            notes: Some("changelog".to_string()),
            skipped: false,
            error: None,
        };
        let notice = update_notice_from_status(&status);
        assert_eq!(
            notice,
            Some(UpdateNotice {
                version: Some("1.5.0".to_string())
            })
        );
    }

    #[test]
    fn update_notice_from_status_hidden_when_unavailable() {
        let status = UpdateStatus::default();
        assert!(update_notice_from_status(&status).is_none());
    }

    #[test]
    fn update_notice_from_status_hidden_when_skipped() {
        // Available but the user dismissed this version → not re-offered in tray.
        let status = UpdateStatus {
            available: true,
            version: Some("1.5.0".to_string()),
            notes: None,
            skipped: true,
            error: None,
        };
        assert!(update_notice_from_status(&status).is_none());
    }

    #[test]
    fn update_notice_carries_no_version_when_missing() {
        let status = UpdateStatus {
            available: true,
            version: None,
            notes: None,
            skipped: false,
            error: None,
        };
        assert_eq!(
            update_notice_from_status(&status),
            Some(UpdateNotice { version: None })
        );
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
