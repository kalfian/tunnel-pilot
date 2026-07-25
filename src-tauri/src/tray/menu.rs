//! Tray menu build + debounced rebuild-on-change, styled after Tailscale /
//! Laravel Herd's native menu-bar menu: a disabled title header ("Tunnel Pilot"
//! plus an "N of M connected" status line) at the top, an update-notice slot, per-
//! tunnel rows with a colored status-dot leading icon bucketed into greyed group
//! section headers (`PRODUCTION` / `STAGING` / `Ungrouped`; a flat list when
//! nothing is grouped), conditional bulk Start/Stop All, and an actions footer
//! (Settings ⌘, / Check for Updates… ⌘U / Quit ⌘Q).
//!
//! The menu *model* ([`build_menu_model`]) is a pure function of the tunnel
//! states + group definitions + update availability, so the "which
//! rows/sections/actions/bulk items appear" logic is unit-testable without a
//! display. Turning the model into a real
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

use super::animation::ConnectingAnimator;
use crate::events;
use crate::state::models::{ForwardStatus, TunnelGroup, UpdateStatus};
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
    /// The group this tunnel belongs to (`None` = ungrouped). Drives the
    /// Herd/Tailscale-style section headers in the tray menu.
    pub group_id: Option<String>,
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
    /// The group this row belongs to (`None` = ungrouped), used to bucket rows
    /// into [`MenuSection`]s.
    pub group_id: Option<String>,
    /// Actions available in this status (empty for the transient `disconnecting`).
    pub actions: Vec<TunnelAction>,
    /// The single action a click on this row performs (`None` while transient).
    pub primary: Option<TunnelAction>,
}

/// A group of per-tunnel rows under an optional section header. `header` is
/// `None` for the single flat section used when no tunnel is grouped; otherwise
/// it is the group name (or `"Ungrouped"`), rendered as a disabled/greyed
/// header item (Herd/Tailscale style).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuSection {
    pub header: Option<String>,
    pub rows: Vec<TunnelMenuRow>,
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
    /// Per-tunnel rows bucketed into group sections (display order). A single
    /// `header: None` section when no tunnel is grouped (flat list).
    pub sections: Vec<MenuSection>,
    /// Connected tunnel count — drives the header status summary + icon badge.
    pub connected: usize,
    /// Total configured tunnel count — drives the header status summary.
    pub total: usize,
    /// "Start All" shown when at least one tunnel is startable.
    pub show_start_all: bool,
    /// "Stop All" shown when at least one tunnel is stoppable.
    pub show_stop_all: bool,
}

impl MenuModel {
    /// All per-tunnel rows, flattened across sections in display order.
    pub fn rows(&self) -> Vec<&TunnelMenuRow> {
        self.sections.iter().flat_map(|s| s.rows.iter()).collect()
    }
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

/// Build the pure tray-menu model from the current tunnel states, group
/// definitions, and update availability. Bulk items appear conditionally: Start
/// All when anything is startable (disconnected/error), Stop All when anything is
/// stoppable (connected/connecting). Rows are bucketed into group sections (see
/// [`build_sections`]) — a flat list when no tunnel is grouped.
pub fn build_menu_model(
    tunnels: &[TunnelState],
    groups: &[TunnelGroup],
    update_notice: Option<UpdateNotice>,
) -> MenuModel {
    let rows: Vec<TunnelMenuRow> = tunnels
        .iter()
        .map(|t| TunnelMenuRow {
            id: t.id.clone(),
            name: t.name.clone(),
            port: t.port,
            status: t.status,
            group_id: t.group_id.clone(),
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
        sections: build_sections(rows, groups),
        connected: connected_count(tunnels),
        total: tunnels.len(),
        show_start_all,
        show_stop_all,
    }
}

/// Bucket rows into group sections (Herd/Tailscale style). When no row carries a
/// `group_id` the result is a single flat section (`header: None`), so an app
/// with no groups renders a plain list. Otherwise rows are grouped by their
/// group (headers in group `order`), and any ungrouped rows — including rows
/// whose `group_id` no longer resolves to a known group — fall under a final
/// `"Ungrouped"` section. Empty group sections (no matching rows) are omitted,
/// and within each section rows keep their incoming display order.
fn build_sections(rows: Vec<TunnelMenuRow>, groups: &[TunnelGroup]) -> Vec<MenuSection> {
    if !rows.iter().any(|r| r.group_id.is_some()) {
        return vec![MenuSection { header: None, rows }];
    }

    let known: std::collections::HashSet<&str> = groups.iter().map(|g| g.id.as_str()).collect();

    let mut ordered: Vec<&TunnelGroup> = groups.iter().collect();
    ordered.sort_by_key(|g| g.order);

    let mut sections: Vec<MenuSection> = Vec::new();
    for group in ordered {
        let group_rows: Vec<TunnelMenuRow> = rows
            .iter()
            .filter(|r| r.group_id.as_deref() == Some(group.id.as_str()))
            .cloned()
            .collect();
        if !group_rows.is_empty() {
            sections.push(MenuSection {
                header: Some(group.name.clone()),
                rows: group_rows,
            });
        }
    }

    let ungrouped: Vec<TunnelMenuRow> = rows
        .into_iter()
        .filter(|r| match &r.group_id {
            None => true,
            Some(id) => !known.contains(id.as_str()),
        })
        .collect();
    if !ungrouped.is_empty() {
        sections.push(MenuSection {
            header: Some("Ungrouped".to_string()),
            rows: ungrouped,
        });
    }

    sections
}

/// Number of tunnels in `Connected` — drives the tray badge count.
pub fn connected_count(tunnels: &[TunnelState]) -> usize {
    tunnels
        .iter()
        .filter(|t| t.status == ForwardStatus::Connected)
        .count()
}

/// Whether any tunnel is in a transitional state (connecting or disconnecting)
/// — drives the tray's connecting loading-dots indicator, which takes precedence
/// over the connected-count badge so the user always sees activity.
pub fn has_transitional(tunnels: &[TunnelState]) -> bool {
    tunnels.iter().any(|t| {
        matches!(
            t.status,
            ForwardStatus::Connecting | ForwardStatus::Disconnecting
        )
    })
}

/// The Tailscale-style header status line: `"No active tunnels"` when none are
/// connected, `"{n} connected"` when all are, else `"{n} of {m} connected"`.
pub fn status_summary(connected: usize, total: usize) -> String {
    if connected == 0 {
        "No active tunnels".to_string()
    } else if connected == total {
        format!("{connected} connected")
    } else {
        format!("{connected} of {total} connected")
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
                group_id: c.group_id,
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
    let groups = state.groups_snapshot();
    let update_notice = gather_update_notice(app);
    let model = build_menu_model(&tunnels, &groups, update_notice);
    build_tauri_menu(app, &model)
}

/// Build a concrete `tauri::menu::Menu` from the pure model. Must run on the
/// main thread (AppKit) — callers dispatch via `run_on_main_thread`.
pub fn build_tauri_menu(app: &AppHandle, model: &MenuModel) -> tauri::Result<Menu<Wry>> {
    // Boxed so heterogeneous item kinds (MenuItem/Submenu/separator) share a Vec.
    let mut items: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();

    // Tailscale-style title header (top): the app name + a disabled status
    // summary line, so the menu opens with a clear title rather than straight
    // into the list. Both are disabled/greyed (standard NSMenu title pattern).
    items.push(Box::new(MenuItem::with_id(
        app,
        "noop:title",
        "Tunnel Pilot",
        false,
        None::<&str>,
    )?));
    items.push(Box::new(MenuItem::with_id(
        app,
        "noop:summary",
        status_summary(model.connected, model.total),
        false,
        None::<&str>,
    )?));
    items.push(Box::new(PredefinedMenuItem::separator(app)?));

    // Update-notice slot (below the title). Present only when an update is
    // pending install.
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
    // status-dot leading icon. Clicking runs the row's primary action. Rows are
    // bucketed into group sections, each led by a disabled/greyed header
    // (Herd-style); a flat list when nothing is grouped.
    let total_rows: usize = model.sections.iter().map(|s| s.rows.len()).sum();
    if total_rows == 0 {
        items.push(Box::new(MenuItem::with_id(
            app,
            "noop:empty",
            "No forwards configured",
            false,
            None::<&str>,
        )?));
    } else {
        let mut first_section = true;
        for section in &model.sections {
            if section.rows.is_empty() {
                continue;
            }
            if let Some(header) = &section.header {
                // Separate consecutive group sections with a divider.
                if !first_section {
                    items.push(Box::new(PredefinedMenuItem::separator(app)?));
                }
                items.push(Box::new(MenuItem::with_id(
                    app,
                    format!("noop:hdr:{header}"),
                    header.clone(),
                    false,
                    None::<&str>,
                )?));
            }
            for row in &section.rows {
                items.push(build_tunnel_row(app, row)?);
            }
            first_section = false;
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
///
/// Icon precedence: any transitional tunnel (connecting/disconnecting) → the
/// connecting ticking-dots badge (owned by `animator`, so the static icon is
/// *not* repainted here); else the connected-count badge / idle. Toggling the
/// ticker is idempotent, so this can be called on every status change.
pub fn rebuild_now(app: &AppHandle, state: &Arc<AppState>, animator: &ConnectingAnimator) {
    let tunnels = gather_tunnel_states(state);
    let groups = state.groups_snapshot();
    let count = connected_count(&tunnels);
    let transitional = has_transitional(&tunnels);
    let update_notice = gather_update_notice(app);
    let model = build_menu_model(&tunnels, &groups, update_notice);

    // Drive the connecting ticker before dispatching the (menu-only when
    // connecting) main-thread paint. `set_active` is idempotent and never
    // spawns/joins a task, so a status event mid-connect can't kill the timer;
    // clearing it first lets the `paint_frame` guard drop any late frame so the
    // static settle below wins.
    animator.set_active(transitional);

    let app_main = app.clone();
    let dispatch = app.run_on_main_thread(move || {
        // While connecting the ticker task owns the icon; only settle the static
        // count/idle icon when nothing is transitional.
        if !transitional {
            super::icon::update_tray_icon(&app_main, super::TRAY_ID, count);
        }
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
    // Single shared animator with one long-lived ticker task (spawned here once).
    // The debounce loop and the initial paint only toggle its `active` flag, so
    // the timer keeps ticking for the whole connect and can never double-run.
    let animator = ConnectingAnimator::new();
    animator.spawn(app.clone());

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

    // Group mutations (create/rename/delete/reorder) change the section headers
    // the menu builds from `groups_snapshot`, so rebuild the tray on them too —
    // otherwise a new/renamed group never appears until an unrelated status
    // change. Coalesced through the same debounce.
    let dirty_groups = notify.clone();
    app.listen(events::GROUPS_CHANGED, move |_event| {
        dirty_groups.notify_one();
    });

    // Forward mutations (add/edit/delete/reorder, and crucially group
    // *assignment* changes) change which section a tunnel row lands under, so
    // the tray must rebuild on them as well. Coalesced through the same debounce.
    let dirty_forwards = notify.clone();
    app.listen(events::FORWARDS_CHANGED, move |_event| {
        dirty_forwards.notify_one();
    });

    // Debounce loop: wake on the first change, wait out the window (coalescing
    // further changes), then do exactly one rebuild.
    let debounce_app = app.clone();
    let debounce_state = state.clone();
    let debounce_animator = animator.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            notify.notified().await;
            tokio::time::sleep(REBUILD_DEBOUNCE).await;
            rebuild_now(&debounce_app, &debounce_state, &debounce_animator);
        }
    });

    // Initial build so the menu reflects the boot state immediately.
    rebuild_now(&app, &state, &animator);
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
            group_id: None,
        }
    }

    /// A tunnel state assigned to `group_id`.
    fn ts_grouped(id: &str, status: ForwardStatus, group_id: &str) -> TunnelState {
        TunnelState {
            group_id: Some(group_id.to_string()),
            ..ts(id, status)
        }
    }

    fn group(id: &str, name: &str, order: u32) -> TunnelGroup {
        TunnelGroup {
            id: id.to_string(),
            name: name.to_string(),
            color: None,
            order,
            collapsed: false,
        }
    }

    #[test]
    fn empty_has_no_bulk() {
        let m = build_menu_model(&[], &[], None);
        assert!(m.rows().is_empty());
        assert!(!m.show_start_all);
        assert!(!m.show_stop_all);
        assert!(m.update_notice.is_none());
    }

    #[test]
    fn error_row_exposes_retry() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Error)], &[], None);
        let rows = m.rows();
        assert!(rows[0].actions.contains(&TunnelAction::Retry));
    }

    #[test]
    fn connected_row_offers_disconnect_only() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Connected)], &[], None);
        assert_eq!(m.rows()[0].actions, vec![TunnelAction::Disconnect]);
    }

    #[test]
    fn disconnected_row_offers_connect_only() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Disconnected)], &[], None);
        assert_eq!(m.rows()[0].actions, vec![TunnelAction::Connect]);
    }

    #[test]
    fn disconnecting_row_has_no_actions() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Disconnecting)], &[], None);
        assert!(m.rows()[0].actions.is_empty());
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
        let m = build_menu_model(&[ts("a", ForwardStatus::Error)], &[], None);
        assert_eq!(m.rows()[0].primary, Some(TunnelAction::Retry));
    }

    #[test]
    fn disconnecting_row_has_no_primary() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Disconnecting)], &[], None);
        assert!(m.rows()[0].primary.is_none());
    }

    #[test]
    fn row_carries_port_and_label() {
        let mut t = ts("a", ForwardStatus::Connected);
        t.name = "Qismo Prod".to_string();
        t.port = 5431;
        let m = build_menu_model(&[t], &[], None);
        let rows = m.rows();
        let row = rows[0];
        assert_eq!(row.port, 5431);
        assert_eq!(tunnel_row_label(row), "Qismo Prod   :5431");
    }

    #[test]
    fn disconnecting_label_includes_state() {
        let mut t = ts("a", ForwardStatus::Disconnecting);
        t.name = "DB".to_string();
        t.port = 6001;
        let m = build_menu_model(&[t], &[], None);
        assert_eq!(
            tunnel_row_label(m.rows()[0]),
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
            &[],
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
            &[],
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
            &[],
            None,
        );
        assert!(m.show_start_all);
        assert!(!m.show_stop_all);
    }

    #[test]
    fn error_makes_start_all_available() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Error)], &[], None);
        assert!(m.show_start_all);
        assert!(!m.show_stop_all);
    }

    #[test]
    fn connecting_makes_stop_all_available() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Connecting)], &[], None);
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
    fn has_transitional_detects_connecting_and_disconnecting() {
        // No transitional tunnels → false (connected/error/disconnected only).
        let settled = [
            ts("a", ForwardStatus::Connected),
            ts("b", ForwardStatus::Error),
            ts("c", ForwardStatus::Disconnected),
        ];
        assert!(!has_transitional(&settled));

        // A single connecting tunnel flips it on, even amid connected ones.
        let connecting = [
            ts("a", ForwardStatus::Connected),
            ts("b", ForwardStatus::Connecting),
        ];
        assert!(has_transitional(&connecting));

        // Disconnecting is transitional too.
        let disconnecting = [ts("a", ForwardStatus::Disconnecting)];
        assert!(has_transitional(&disconnecting));

        // Empty set → nothing transitional.
        assert!(!has_transitional(&[]));
    }

    // --- header status summary ---------------------------------------------

    #[test]
    fn summary_none_connected() {
        assert_eq!(status_summary(0, 0), "No active tunnels");
        assert_eq!(status_summary(0, 3), "No active tunnels");
    }

    #[test]
    fn summary_all_connected() {
        assert_eq!(status_summary(3, 3), "3 connected");
    }

    #[test]
    fn summary_partial() {
        assert_eq!(status_summary(1, 3), "1 of 3 connected");
    }

    #[test]
    fn model_carries_connected_and_total() {
        let m = build_menu_model(
            &[
                ts("a", ForwardStatus::Connected),
                ts("b", ForwardStatus::Disconnected),
            ],
            &[],
            None,
        );
        assert_eq!(m.connected, 1);
        assert_eq!(m.total, 2);
    }

    // --- group sections -----------------------------------------------------

    #[test]
    fn flat_section_when_no_groups() {
        let m = build_menu_model(
            &[
                ts("a", ForwardStatus::Connected),
                ts("b", ForwardStatus::Disconnected),
            ],
            &[],
            None,
        );
        assert_eq!(m.sections.len(), 1);
        assert!(m.sections[0].header.is_none());
        assert_eq!(m.sections[0].rows.len(), 2);
    }

    #[test]
    fn grouped_rows_get_section_headers_in_group_order() {
        // Groups declared out of order; sections must follow `order`.
        let groups = [group("g2", "Staging", 1), group("g1", "Production", 0)];
        let m = build_menu_model(
            &[
                ts_grouped("a", ForwardStatus::Connected, "g2"),
                ts_grouped("b", ForwardStatus::Connected, "g1"),
            ],
            &groups,
            None,
        );
        assert_eq!(m.sections.len(), 2);
        assert_eq!(m.sections[0].header.as_deref(), Some("Production"));
        assert_eq!(m.sections[0].rows[0].id, "b");
        assert_eq!(m.sections[1].header.as_deref(), Some("Staging"));
        assert_eq!(m.sections[1].rows[0].id, "a");
    }

    #[test]
    fn ungrouped_rows_fall_under_ungrouped_section_last() {
        let groups = [group("g1", "Production", 0)];
        let m = build_menu_model(
            &[
                ts("a", ForwardStatus::Connected),
                ts_grouped("b", ForwardStatus::Connected, "g1"),
            ],
            &groups,
            None,
        );
        assert_eq!(m.sections.len(), 2);
        assert_eq!(m.sections[0].header.as_deref(), Some("Production"));
        assert_eq!(m.sections[1].header.as_deref(), Some("Ungrouped"));
        assert_eq!(m.sections[1].rows[0].id, "a");
    }

    #[test]
    fn unknown_group_id_falls_back_to_ungrouped() {
        // A row referencing a group that no longer exists renders as ungrouped,
        // never dropped.
        let groups = [group("g1", "Production", 0)];
        let m = build_menu_model(
            &[ts_grouped("a", ForwardStatus::Connected, "ghost")],
            &groups,
            None,
        );
        assert_eq!(m.sections.len(), 1);
        assert_eq!(m.sections[0].header.as_deref(), Some("Ungrouped"));
        assert_eq!(m.rows().len(), 1);
    }

    #[test]
    fn empty_group_section_is_omitted() {
        // A declared group with no rows must not produce a header.
        let groups = [group("g1", "Production", 0), group("g2", "Staging", 1)];
        let m = build_menu_model(
            &[ts_grouped("a", ForwardStatus::Connected, "g1")],
            &groups,
            None,
        );
        assert_eq!(m.sections.len(), 1);
        assert_eq!(m.sections[0].header.as_deref(), Some("Production"));
    }

    #[test]
    fn update_notice_flows_through_when_present() {
        let notice = UpdateNotice {
            version: Some("1.5.0".to_string()),
        };
        let m = build_menu_model(&[], &[], Some(notice.clone()));
        assert_eq!(m.update_notice, Some(notice));
    }

    #[test]
    fn update_notice_absent_when_none() {
        let m = build_menu_model(&[ts("a", ForwardStatus::Connected)], &[], None);
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
