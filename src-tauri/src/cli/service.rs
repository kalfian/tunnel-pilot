//! Request handlers for the control socket — the only place the CLI touches
//! app state (spec 03 §20).
//!
//! Every mutation goes through the SAME service functions the tray and the
//! webview use (`ssh::engine::*`, `ops::forwards::*`), so a CLI-driven connect
//! or edit emits its event, refreshes the tray, and is visible to the frontend
//! on its next hydrate. The CLI is a second *driver*, never a second state
//! owner — whether the owner is the desktop app or the headless daemon.
//!
//! No secret ever reaches this module: `password-set` only flips the
//! `hasStoredPassword` flag (the CLI process wrote the secret itself) and
//! `password-clear` deletes without reading (AGENTS §8).

use std::sync::Arc;

use tokio::sync::watch;
use tokio::time::{timeout_at, Duration, Instant};

use crate::cli::protocol::{
    BulkResult, CliCommand, CliData, CliRequest, CliResponse, ForwardFields, ForwardView,
    SaveChange, PROTOCOL_VERSION,
};
use crate::error::AppError;
use crate::ops::forwards::{self as ops, run_start_all, run_stop_all, runtime_or_default};
use crate::ssh::engine;
use crate::state::models::{ForwardConfig, ForwardInput, ForwardStatus};
use crate::state::AppState;

/// How long to wait for a supervisor handle to appear in the registry after
/// `connect_forward` returns. It returns `Ok(())` when it loses the F33
/// start race, so "no handle yet" is legal for a short window.
const HANDLE_GRACE: Duration = Duration::from_secs(1);
/// Poll cadence inside [`HANDLE_GRACE`] (the only polling in the wait path —
/// the status itself is awaited on the existing `watch` channel).
const HANDLE_POLL: Duration = Duration::from_millis(50);

/// Result of awaiting a terminal status for one tunnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// Reached `connected` or `error`.
    Terminal(ForwardStatus),
    /// The wait budget elapsed while still `connecting`.
    TimedOut,
    /// The handle disappeared (disconnected/deleted mid-wait).
    Vanished,
}

/// Handle one request line. Never returns `Err`: a failure is a `CliResponse`
/// carrying the app's own [`AppError`], which the client maps to an exit code.
pub async fn handle_request(state: &Arc<AppState>, req: CliRequest) -> CliResponse {
    if req.v != PROTOCOL_VERSION {
        return CliResponse::err(AppError::InvalidInput(format!(
            "unsupported protocol version {}; this app speaks v{PROTOCOL_VERSION}",
            req.v
        )));
    }
    match dispatch(state, req).await {
        Ok(data) => CliResponse::ok(data),
        Err(e) => CliResponse::err(e),
    }
}

async fn dispatch(state: &Arc<AppState>, req: CliRequest) -> Result<CliData, AppError> {
    let timeout_ms = req.effective_timeout_ms();
    match req.cmd {
        CliCommand::List => Ok(list(state)),
        CliCommand::Version => Ok(CliData::Version {
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol: PROTOCOL_VERSION,
        }),
        CliCommand::Status => status(state, &require_target(req.target)?),
        CliCommand::Connect => {
            connect(
                state,
                &require_target(req.target)?,
                req.wait,
                timeout_ms,
                req.force,
            )
            .await
        }
        CliCommand::Disconnect => disconnect(state, &require_target(req.target)?).await,
        CliCommand::ConnectAll => connect_all(state, req.wait, timeout_ms).await,
        CliCommand::DisconnectAll => disconnect_all(state).await,
        CliCommand::Add => add(state, require_fields(req.forward)?).await,
        CliCommand::Edit => {
            let target = require_target(req.target)?;
            edit(state, &target, require_fields(req.forward)?).await
        }
        CliCommand::Remove => remove(state, &require_target(req.target)?).await,
        CliCommand::PasswordSet => password_flag(state, &require_target(req.target)?, true).await,
        CliCommand::PasswordClear => {
            password_flag(state, &require_target(req.target)?, false).await
        }
        CliCommand::HostKeyTrust => {
            let cfg = resolve_target(state, &require_target(req.target)?)?;
            let cfg = ops::trust_host_key(state, &cfg.id).await?;
            Ok(saved(state, &cfg, SaveChange::HostKeyTrusted))
        }
        CliCommand::HostKeyForget => {
            let cfg = resolve_target(state, &require_target(req.target)?)?;
            let cfg = ops::forget_host_key(state, &cfg.id).await?;
            Ok(saved(state, &cfg, SaveChange::HostKeyForgotten))
        }
    }
}

fn require_fields(fields: Option<ForwardFields>) -> Result<ForwardFields, AppError> {
    fields.ok_or_else(|| AppError::InvalidInput("this command requires forward fields".into()))
}

// --- CRUD (spec 03 §20): the same `ops::forwards` path the window uses ---

/// The defaults a new forward gets for anything `add` did not specify — the
/// same values the desktop form pre-fills (spec 04 §1).
fn default_input() -> ForwardInput {
    ForwardInput {
        name: String::new(),
        ssh_host: String::new(),
        ssh_port: 22,
        ssh_username: String::new(),
        identity_file_path: None,
        local_bind_address: "127.0.0.1".into(),
        local_port: 0,
        remote_host: String::new(),
        remote_port: 0,
        keep_alive_interval_sec: 30,
        keep_alive_max_count: 5,
        group_id: None,
        tags: Vec::new(),
    }
}

/// Overlay the fields that are present onto `input`. Pure — unit-tested.
pub(crate) fn apply_fields(mut input: ForwardInput, fields: ForwardFields) -> ForwardInput {
    if let Some(v) = fields.name {
        input.name = v.trim().to_string();
    }
    if let Some(v) = fields.ssh_host {
        input.ssh_host = v.trim().to_string();
    }
    if let Some(v) = fields.ssh_port {
        input.ssh_port = v;
    }
    if let Some(v) = fields.ssh_username {
        input.ssh_username = v.trim().to_string();
    }
    if let Some(v) = fields.identity_file_path {
        let v = v.trim();
        input.identity_file_path = (!v.is_empty()).then(|| v.to_string());
    }
    if let Some(v) = fields.local_bind_address {
        input.local_bind_address = v.trim().to_string();
    }
    if let Some(v) = fields.local_port {
        input.local_port = v;
    }
    if let Some(v) = fields.remote_host {
        input.remote_host = v.trim().to_string();
    }
    if let Some(v) = fields.remote_port {
        input.remote_port = v;
    }
    if let Some(v) = fields.keep_alive_interval_sec {
        input.keep_alive_interval_sec = v;
    }
    if let Some(v) = fields.keep_alive_max_count {
        input.keep_alive_max_count = v;
    }
    if let Some(v) = fields.tags {
        input.tags = v;
    }
    input
}

/// The desktop form's rules (`src/lib/validation.ts`), minus the auth rule —
/// the CLI enforces "password XOR identity" before it sends, because only the
/// client knows whether a password is about to be stored. Pure — unit-tested.
pub(crate) fn validate_input(input: &ForwardInput) -> Result<(), AppError> {
    let mut problems: Vec<&str> = Vec::new();
    if input.name.trim().is_empty() {
        problems.push("name is required");
    }
    if input.ssh_host.trim().is_empty() {
        problems.push("SSH host is required");
    }
    if input.ssh_username.trim().is_empty() {
        problems.push("SSH username is required");
    }
    if input.local_bind_address.trim().is_empty() {
        problems.push("local bind address is required");
    }
    if input.remote_host.trim().is_empty() {
        problems.push("remote host is required");
    }
    // u16 already caps at 65535; 0 is the only out-of-range value.
    if input.ssh_port == 0 {
        problems.push("SSH port must be between 1 and 65535");
    }
    if input.local_port == 0 {
        problems.push("local port must be between 1 and 65535");
    }
    if input.remote_port == 0 {
        problems.push("remote port must be between 1 and 65535");
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(AppError::InvalidInput(problems.join("; ")))
    }
}

/// A CLI-created or renamed forward must stay addressable by name, so a name
/// already used by ANOTHER forward (case-insensitive) is refused. The desktop
/// form does not enforce this; the CLI does because names are its targets.
fn ensure_unique_name(
    state: &Arc<AppState>,
    name: &str,
    except_id: Option<&str>,
) -> Result<(), AppError> {
    let wanted = name.to_lowercase();
    let clash = state
        .configs_snapshot()
        .into_iter()
        .any(|c| Some(c.id.as_str()) != except_id && c.name.to_lowercase() == wanted);
    if clash {
        return Err(AppError::InvalidInput(format!(
            "a forward named '{name}' already exists; pick another name"
        )));
    }
    Ok(())
}

async fn add(state: &Arc<AppState>, fields: ForwardFields) -> Result<CliData, AppError> {
    let input = apply_fields(default_input(), fields);
    validate_input(&input)?;
    ensure_unique_name(state, &input.name, None)?;
    let cfg = ops::create(state, input).await?;
    Ok(saved(state, &cfg, SaveChange::Created))
}

async fn edit(
    state: &Arc<AppState>,
    target: &str,
    fields: ForwardFields,
) -> Result<CliData, AppError> {
    if fields.is_empty() {
        return Err(AppError::InvalidInput("nothing to change".into()));
    }
    let existing = resolve_target(state, target)?;
    let input = apply_fields(ops::input_from_config(&existing), fields);
    validate_input(&input)?;
    ensure_unique_name(state, &input.name, Some(&existing.id))?;
    let cfg = ops::update(state, existing.id, input).await?;
    Ok(saved(state, &cfg, SaveChange::Updated))
}

async fn remove(state: &Arc<AppState>, target: &str) -> Result<CliData, AppError> {
    let cfg = resolve_target(state, target)?;
    ops::delete(state, &cfg.id).await?;
    // The forward is gone: report its last config as disconnected.
    Ok(saved(state, &cfg, SaveChange::Removed))
}

async fn password_flag(
    state: &Arc<AppState>,
    target: &str,
    stored: bool,
) -> Result<CliData, AppError> {
    let cfg = resolve_target(state, target)?;
    let change = if stored {
        ops::mark_password_stored(state, &cfg.id, true).await?;
        SaveChange::PasswordSet
    } else {
        ops::clear_password(state, &cfg.id).await?;
        SaveChange::PasswordCleared
    };
    let cfg = state.get_config(&cfg.id).unwrap_or(cfg);
    Ok(saved(state, &cfg, change))
}

fn saved(state: &Arc<AppState>, cfg: &ForwardConfig, change: SaveChange) -> CliData {
    CliData::Saved {
        forward: view(state, cfg),
        change,
    }
}

fn require_target(target: Option<String>) -> Result<String, AppError> {
    target.ok_or_else(|| AppError::InvalidInput("this command requires a target".into()))
}

/// Resolve a user-supplied target to exactly one forward: exact id first
/// (case-sensitive uuid), then case-insensitive exact name.
///
/// Deliberately NO fuzzy or prefix matching — an agent must never connect the
/// wrong tunnel because a name happened to be a prefix of another.
pub fn resolve_target(state: &Arc<AppState>, target: &str) -> Result<ForwardConfig, AppError> {
    let configs = state.configs_snapshot();
    if let Some(cfg) = configs.iter().find(|c| c.id == target) {
        return Ok(cfg.clone());
    }
    let wanted = target.to_lowercase();
    let matches: Vec<&ForwardConfig> = configs
        .iter()
        .filter(|c| c.name.to_lowercase() == wanted)
        .collect();
    match matches.as_slice() {
        [] => Err(AppError::NotFound(format!(
            "no forward matches '{target}' (by id or name)"
        ))),
        [only] => Ok((*only).clone()),
        many => Err(AppError::InvalidInput(format!(
            "ambiguous target '{target}': {} forwards match; use the id",
            many.len()
        ))),
    }
}

/// Build the wire view for a config from the live registry runtime.
fn view(state: &Arc<AppState>, cfg: &ForwardConfig) -> ForwardView {
    ForwardView::from_parts(cfg, &runtime_or_default(state, &cfg.id))
}

fn list(state: &Arc<AppState>) -> CliData {
    let forwards = state
        .configs_snapshot()
        .iter()
        .map(|cfg| view(state, cfg))
        .collect();
    CliData::List { forwards }
}

fn status(state: &Arc<AppState>, target: &str) -> Result<CliData, AppError> {
    let cfg = resolve_target(state, target)?;
    let runtime = runtime_or_default(state, &cfg.id);
    Ok(CliData::Status {
        forward: ForwardView::from_parts(&cfg, &runtime),
        stats: runtime.stats,
    })
}

/// Connect one forward — **idempotent unless `force`**.
///
/// `engine::connect_forward` disconnects a live tunnel before re-dialing
/// (`ssh/engine.rs`, "already-connected same id"), which would kill every TCP
/// session through the local port. A defensive `tunnel-pilot connect prod-db`
/// must not do that, so an already-`connected` tunnel is reported unchanged and
/// a `connecting` one is only awaited — the engine is not touched at all. This
/// matches `run_start_all` (which skips connected/connecting), so `connect` and
/// `connect-all` agree. `--force` (`force: true`) restores the bounce.
async fn connect(
    state: &Arc<AppState>,
    target: &str,
    wait: bool,
    timeout_ms: u64,
    force: bool,
) -> Result<CliData, AppError> {
    let cfg = resolve_target(state, target)?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);

    if needs_dial(force, state.registry.current_status(&cfg.id)) {
        engine::connect_forward(state, &cfg.id).await?;
    }

    // Safe for the no-dial paths too: an already-`connected` tunnel makes
    // `wait_for_terminal` return immediately, and a `connecting` one is simply
    // joined rather than restarted.
    let timed_out = wait
        && matches!(
            wait_for_terminal(state, &cfg.id, deadline).await,
            WaitOutcome::TimedOut
        );

    Ok(CliData::Action {
        forward: view(state, &cfg),
        waited: wait,
        timed_out,
    })
}

/// Whether a `connect` request must go through `engine::connect_forward`.
///
/// Pure so the idempotency rule is testable without dialing anything. `true`
/// means "the engine may tear down and re-dial"; the guard exists because
/// `connect_forward` disconnects a live tunnel first.
pub(crate) fn needs_dial(force: bool, current: Option<ForwardStatus>) -> bool {
    force
        || !matches!(
            current,
            Some(ForwardStatus::Connected) | Some(ForwardStatus::Connecting)
        )
}

async fn disconnect(state: &Arc<AppState>, target: &str) -> Result<CliData, AppError> {
    let cfg = resolve_target(state, target)?;
    // `disconnect_forward` awaits the supervisor's join before returning, so
    // the teardown is synchronous by construction — no wait loop needed.
    engine::disconnect_forward(state, &cfg.id, true).await?;
    Ok(CliData::Action {
        forward: view(state, &cfg),
        waited: true,
        timed_out: false,
    })
}

async fn connect_all(
    state: &Arc<AppState>,
    wait: bool,
    timeout_ms: u64,
) -> Result<CliData, AppError> {
    // The exact tray/palette path (AGENTS §1 — never a loop over per-tunnel
    // connects in a second implementation).
    run_start_all(state).await?;
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut results = Vec::new();
    for cfg in state.configs_snapshot() {
        // All tunnels share ONE deadline, so `connect-all --timeout 30` is a
        // 30s budget for the sweep, not 30s per tunnel.
        let timed_out = wait
            && matches!(
                wait_for_terminal(state, &cfg.id, deadline).await,
                WaitOutcome::TimedOut
            );
        results.push(bulk_result(state, &cfg, timed_out));
    }
    Ok(summarize(results, ForwardStatus::Connected, wait))
}

async fn disconnect_all(state: &Arc<AppState>) -> Result<CliData, AppError> {
    run_stop_all(state).await?;
    let results = state
        .configs_snapshot()
        .iter()
        .map(|cfg| bulk_result(state, cfg, false))
        .collect();
    // `run_stop_all` awaits every supervisor join, so the sweep is synchronous
    // by construction — anything not `disconnected` afterwards is a failure.
    Ok(summarize(results, ForwardStatus::Disconnected, true))
}

fn bulk_result(state: &Arc<AppState>, cfg: &ForwardConfig, timed_out: bool) -> BulkResult {
    let runtime = runtime_or_default(state, &cfg.id);
    BulkResult {
        id: cfg.id.clone(),
        name: cfg.name.clone(),
        status: runtime.status,
        last_error: runtime.last_error,
        timed_out,
    }
}

/// Count a bulk sweep: `succeeded` = reached `wanted`.
///
/// `failed` depends on whether the caller asked us to wait. **With** a wait, the
/// sweep is over and anything short of `wanted` failed — including a member left
/// `disconnected`, which otherwise slips through as neither succeeded nor failed
/// and lets `connect-all` exit 0 having connected nothing. **Without** a wait
/// (`--no-wait`), only `error`/`timedOut` count: a tunnel still `connecting` has
/// an unknown outcome because the caller asked not to find out.
fn summarize(results: Vec<BulkResult>, wanted: ForwardStatus, waited: bool) -> CliData {
    let succeeded = results.iter().filter(|r| r.status == wanted).count();
    let failed = results
        .iter()
        .filter(|r| {
            r.status == ForwardStatus::Error || r.timed_out || (waited && r.status != wanted)
        })
        .count();
    CliData::Bulk {
        results,
        succeeded,
        failed,
    }
}

/// Await a terminal status (`connected` / `error`) for one tunnel.
///
/// `connect_forward` returns as soon as the supervisor is spawned — the status
/// is still `disconnected`/`connecting` at that point — so the CLI's
/// synchronous contract lives here. The existing per-tunnel `watch` channel is
/// the wakeup source; the only polling is the short grace loop waiting for the
/// handle itself to appear.
pub async fn wait_for_terminal(state: &Arc<AppState>, id: &str, deadline: Instant) -> WaitOutcome {
    let Some(mut rx) = await_handle(state, id, deadline).await else {
        return if Instant::now() >= deadline {
            WaitOutcome::TimedOut
        } else {
            WaitOutcome::Vanished
        };
    };

    let mut resubscribed = false;
    loop {
        let current = *rx.borrow_and_update();
        if is_terminal(current) {
            return WaitOutcome::Terminal(current);
        }
        match timeout_at(deadline, rx.changed()).await {
            Err(_) => return WaitOutcome::TimedOut,
            Ok(Ok(())) => continue,
            // Sender dropped: the handle was removed, or replaced by a fresh
            // connect. Re-subscribe ONCE; still gone ⇒ the tunnel vanished.
            Ok(Err(_)) => {
                if resubscribed {
                    return WaitOutcome::Vanished;
                }
                resubscribed = true;
                match state.registry.subscribe_status(id) {
                    Some(fresh) => rx = fresh,
                    None => return WaitOutcome::Vanished,
                }
            }
        }
    }
}

/// Terminal for a connect attempt: the supervisor has either brought the
/// forward up or parked it in `error`.
fn is_terminal(status: ForwardStatus) -> bool {
    matches!(status, ForwardStatus::Connected | ForwardStatus::Error)
}

/// Wait (≤ [`HANDLE_GRACE`], capped by the caller's deadline) for the tunnel's
/// handle to land in the registry, and subscribe to it.
async fn await_handle(
    state: &Arc<AppState>,
    id: &str,
    deadline: Instant,
) -> Option<watch::Receiver<ForwardStatus>> {
    let grace_deadline = std::cmp::min(Instant::now() + HANDLE_GRACE, deadline);
    loop {
        if let Some(rx) = state.registry.subscribe_status(id) {
            return Some(rx);
        }
        if Instant::now() >= grace_deadline {
            return None;
        }
        tokio::time::sleep(HANDLE_POLL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cli::protocol::sample_config;
    use crate::state::tunnel_registry::fake_handle;

    fn bulk(id: &str, status: ForwardStatus) -> BulkResult {
        BulkResult {
            id: id.to_string(),
            name: id.to_string(),
            status,
            last_error: None,
            timed_out: false,
        }
    }

    fn state_with(names: &[(&str, &str)]) -> Arc<AppState> {
        let state = Arc::new(AppState::new_headless());
        for (id, name) in names {
            state.upsert_config(sample_config(id, name));
        }
        state
    }

    #[test]
    fn resolves_by_exact_id() {
        let state = state_with(&[("id-a", "prod-db"), ("id-b", "staging-redis")]);
        let cfg = resolve_target(&state, "id-b").expect("resolve by id");
        assert_eq!(cfg.name, "staging-redis");
    }

    #[test]
    fn resolves_by_case_insensitive_name() {
        let state = state_with(&[("id-a", "Prod-DB")]);
        for target in ["Prod-DB", "prod-db", "PROD-DB"] {
            let cfg = resolve_target(&state, target).expect("resolve by name");
            assert_eq!(cfg.id, "id-a");
        }
    }

    #[test]
    fn unknown_target_is_not_found() {
        let state = state_with(&[("id-a", "prod-db")]);
        let err = resolve_target(&state, "nope").expect_err("must not resolve");
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    }

    #[test]
    fn ambiguous_name_is_rejected_not_guessed() {
        let state = state_with(&[("id-a", "db"), ("id-b", "DB")]);
        let err = resolve_target(&state, "db").expect_err("must not guess");
        match err {
            AppError::InvalidInput(msg) => {
                assert!(msg.contains("ambiguous"), "{msg}");
                assert!(msg.contains("use the id"), "{msg}");
            }
            other => panic!("expected invalidInput, got {other:?}"),
        }
    }

    #[test]
    fn id_match_wins_over_a_name_collision() {
        // A forward named exactly like another forward's id must not shadow it.
        let state = state_with(&[("id-a", "prod-db"), ("id-b", "id-a")]);
        let cfg = resolve_target(&state, "id-a").expect("id match wins");
        assert_eq!(cfg.name, "prod-db");
    }

    #[tokio::test]
    async fn list_reports_live_status_and_disconnected_default() {
        let state = state_with(&[("id-a", "prod-db"), ("id-b", "staging")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connected));

        let CliData::List { forwards } = list(&state) else {
            panic!("expected a list payload");
        };
        assert_eq!(forwards.len(), 2);
        assert_eq!(forwards[0].status, ForwardStatus::Connected);
        assert_eq!(forwards[1].status, ForwardStatus::Disconnected);
    }

    #[tokio::test]
    async fn wait_returns_terminal_when_the_supervisor_connects() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connecting));

        let mover = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            mover
                .registry
                .set_status("id-a", ForwardStatus::Connected, None);
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(
            wait_for_terminal(&state, "id-a", deadline).await,
            WaitOutcome::Terminal(ForwardStatus::Connected)
        );
    }

    #[tokio::test]
    async fn wait_returns_terminal_immediately_for_an_already_terminal_status() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connected));
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(
            wait_for_terminal(&state, "id-a", deadline).await,
            WaitOutcome::Terminal(ForwardStatus::Connected)
        );
    }

    #[tokio::test]
    async fn wait_reports_error_as_a_terminal_outcome() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connecting));
        state
            .registry
            .set_status("id-a", ForwardStatus::Error, Some("auth failed".into()));
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(
            wait_for_terminal(&state, "id-a", deadline).await,
            WaitOutcome::Terminal(ForwardStatus::Error)
        );
    }

    #[tokio::test]
    async fn wait_times_out_while_still_connecting() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connecting));
        // A deadline already in the past must not hang.
        let deadline = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            wait_for_terminal(&state, "id-a", deadline).await,
            WaitOutcome::TimedOut
        );
    }

    #[tokio::test]
    async fn wait_times_out_when_the_handle_never_appears() {
        let state = state_with(&[("id-a", "prod-db")]);
        let deadline = Instant::now() + Duration::from_millis(120);
        assert_eq!(
            wait_for_terminal(&state, "id-a", deadline).await,
            WaitOutcome::TimedOut
        );
    }

    #[tokio::test]
    async fn wait_reports_vanished_when_the_handle_is_removed() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connecting));

        let remover = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            // Dropping the handle drops the watch sender.
            drop(remover.registry.remove("id-a"));
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(
            wait_for_terminal(&state, "id-a", deadline).await,
            WaitOutcome::Vanished
        );
    }

    /// The idempotency rule (finding 1): a live tunnel is never bounced by a
    /// plain `connect`; only `--force` (or a non-live status) reaches the
    /// engine, which tears down and re-dials.
    #[test]
    fn only_force_or_a_non_live_status_reaches_the_engine() {
        for status in [ForwardStatus::Connected, ForwardStatus::Connecting] {
            assert!(
                !needs_dial(false, Some(status)),
                "{status:?} must not be re-dialed"
            );
            assert!(needs_dial(true, Some(status)), "--force must re-dial");
        }
        for status in [
            ForwardStatus::Disconnected,
            ForwardStatus::Disconnecting,
            ForwardStatus::Error,
        ] {
            assert!(needs_dial(false, Some(status)), "{status:?} must dial");
        }
        assert!(needs_dial(false, None), "a non-live tunnel must dial");
    }

    #[tokio::test]
    async fn connect_on_a_connected_tunnel_is_a_no_op_that_reports_connected() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connected));

        let data = connect(&state, "prod-db", true, 5_000, false)
            .await
            .expect("connect");
        let CliData::Action {
            forward,
            waited,
            timed_out,
        } = data
        else {
            panic!("expected an action payload");
        };
        assert_eq!(forward.status, ForwardStatus::Connected);
        assert!(waited && !timed_out);
        // The live handle survived: no teardown, so no TCP session was killed.
        assert!(
            state.registry.contains("id-a"),
            "a plain connect must not bounce a live tunnel"
        );
        assert_eq!(
            state.registry.current_status("id-a"),
            Some(ForwardStatus::Connected)
        );
    }

    #[tokio::test]
    async fn connect_on_a_connecting_tunnel_joins_the_dial_in_flight() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connecting));

        let mover = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            mover
                .registry
                .set_status("id-a", ForwardStatus::Connected, None);
        });

        let data = connect(&state, "id-a", true, 5_000, false)
            .await
            .expect("connect");
        let CliData::Action {
            forward, timed_out, ..
        } = data
        else {
            panic!("expected an action payload");
        };
        assert_eq!(forward.status, ForwardStatus::Connected);
        assert!(!timed_out);
        assert!(
            state.registry.contains("id-a"),
            "the dial was not restarted"
        );
    }

    #[tokio::test]
    async fn connect_with_no_wait_on_a_connecting_tunnel_returns_immediately() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connecting));

        let data = connect(&state, "id-a", false, 5_000, false)
            .await
            .expect("connect");
        let CliData::Action {
            forward,
            waited,
            timed_out,
        } = data
        else {
            panic!("expected an action payload");
        };
        assert_eq!(forward.status, ForwardStatus::Connecting);
        assert!(!waited && !timed_out);
    }

    #[tokio::test]
    async fn disconnect_removes_the_handle_and_reports_disconnected() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connected));

        let data = disconnect(&state, "PROD-DB").await.expect("disconnect");
        let CliData::Action {
            forward,
            waited,
            timed_out,
        } = data
        else {
            panic!("expected an action payload");
        };
        assert_eq!(forward.status, ForwardStatus::Disconnected);
        assert!(waited && !timed_out);
        assert!(!state.registry.contains("id-a"), "handle must be gone");
    }

    #[tokio::test]
    async fn disconnect_all_reports_every_forward_disconnected() {
        let state = state_with(&[("id-a", "prod-db"), ("id-b", "staging")]);
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connected));

        let CliData::Bulk {
            results,
            succeeded,
            failed,
        } = disconnect_all(&state).await.expect("disconnect all")
        else {
            panic!("expected a bulk payload");
        };
        assert_eq!(results.len(), 2);
        assert_eq!(succeeded, 2);
        assert_eq!(failed, 0);
        assert!(state.registry.all_ids().is_empty());
    }

    /// Finding 4b: with a wait requested, a member left `disconnected` is a
    /// failure — otherwise `connect-all` reports "0 succeeded, 0 failed" and
    /// the client exits 0 having connected nothing.
    #[test]
    fn a_waited_sweep_counts_a_disconnected_member_as_failed() {
        let results = vec![
            bulk("id-a", ForwardStatus::Connected),
            bulk("id-b", ForwardStatus::Disconnected),
        ];
        let CliData::Bulk {
            succeeded, failed, ..
        } = summarize(results.clone(), ForwardStatus::Connected, true)
        else {
            panic!("expected a bulk payload");
        };
        assert_eq!((succeeded, failed), (1, 1));

        // `--no-wait`: an unfinished member has an unknown outcome, not a failure.
        let CliData::Bulk {
            succeeded, failed, ..
        } = summarize(
            vec![bulk("id-a", ForwardStatus::Connecting)],
            ForwardStatus::Connected,
            false,
        )
        else {
            panic!("expected a bulk payload");
        };
        assert_eq!((succeeded, failed), (0, 0));
    }

    #[test]
    fn a_timed_out_member_is_counted_once() {
        let mut results = vec![bulk("id-a", ForwardStatus::Connecting)];
        results[0].timed_out = true;
        let CliData::Bulk {
            succeeded, failed, ..
        } = summarize(results, ForwardStatus::Connected, true)
        else {
            panic!("expected a bulk payload");
        };
        assert_eq!((succeeded, failed), (0, 1));
    }

    #[tokio::test]
    async fn unknown_protocol_version_is_rejected_before_any_work() {
        let state = state_with(&[("id-a", "prod-db")]);
        let mut req = CliRequest::new(CliCommand::List);
        req.v = 99;
        let res = handle_request(&state, req).await;
        assert!(!res.ok);
        match res.error {
            Some(AppError::InvalidInput(msg)) => assert!(msg.contains("99"), "{msg}"),
            other => panic!("expected invalidInput, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn commands_that_need_a_target_reject_a_missing_one() {
        let state = state_with(&[("id-a", "prod-db")]);
        for cmd in [
            CliCommand::Status,
            CliCommand::Connect,
            CliCommand::Disconnect,
        ] {
            let res = handle_request(&state, CliRequest::new(cmd)).await;
            assert!(!res.ok, "{cmd:?} must require a target");
            assert!(matches!(res.error, Some(AppError::InvalidInput(_))));
        }
    }

    #[tokio::test]
    async fn version_reports_the_crate_version_and_protocol() {
        let state = state_with(&[]);
        let res = handle_request(&state, CliRequest::new(CliCommand::Version)).await;
        assert!(res.ok);
        match res.data {
            Some(CliData::Version {
                app_version,
                protocol,
            }) => {
                assert_eq!(app_version, env!("CARGO_PKG_VERSION"));
                assert_eq!(protocol, PROTOCOL_VERSION);
            }
            other => panic!("expected a version payload, got {other:?}"),
        }
    }
    // --- CRUD (spec 03 §20) ---

    fn target_req(cmd: CliCommand, target: &str) -> CliRequest {
        CliRequest {
            target: Some(target.to_string()),
            ..CliRequest::new(cmd)
        }
    }

    fn fields(name: &str) -> ForwardFields {
        ForwardFields {
            name: Some(name.into()),
            ssh_host: Some("bastion.example.com".into()),
            ssh_username: Some("deploy".into()),
            identity_file_path: Some("~/.ssh/id_ed25519".into()),
            local_port: Some(15432),
            remote_host: Some("db.internal".into()),
            remote_port: Some(5432),
            ..ForwardFields::default()
        }
    }

    fn with_fields(cmd: CliCommand, target: Option<&str>, f: ForwardFields) -> CliRequest {
        CliRequest {
            target: target.map(str::to_string),
            forward: Some(f),
            ..CliRequest::new(cmd)
        }
    }

    #[test]
    fn apply_fields_overlays_only_present_fields_and_trims() {
        let base = apply_fields(default_input(), fields("db"));
        let edited = apply_fields(
            base.clone(),
            ForwardFields {
                name: Some("  renamed ".into()),
                identity_file_path: Some(String::new()),
                tags: Some(vec!["prod".into()]),
                ..ForwardFields::default()
            },
        );
        assert_eq!(edited.name, "renamed");
        assert_eq!(
            edited.identity_file_path, None,
            "empty string clears identity"
        );
        assert_eq!(edited.tags, vec!["prod".to_string()]);
        // Untouched fields keep their values.
        assert_eq!(edited.ssh_host, base.ssh_host);
        assert_eq!(edited.local_port, 15432);
        assert_eq!(edited.ssh_port, 22, "default ssh port");
        assert_eq!(edited.local_bind_address, "127.0.0.1", "default bind");
    }

    #[test]
    fn validate_input_reports_every_missing_field() {
        let err = validate_input(&default_input()).expect_err("empty input is invalid");
        let AppError::InvalidInput(msg) = err else {
            panic!("expected invalidInput");
        };
        for needle in [
            "name",
            "SSH host",
            "SSH username",
            "remote host",
            "local port",
        ] {
            assert!(msg.contains(needle), "{needle} missing from: {msg}");
        }
        assert!(validate_input(&apply_fields(default_input(), fields("db"))).is_ok());
    }

    #[tokio::test]
    async fn add_edit_remove_round_trip_through_the_socket_handler() {
        let state = state_with(&[]);

        let res = handle_request(&state, with_fields(CliCommand::Add, None, fields("db"))).await;
        let Some(CliData::Saved { forward, change }) = res.data else {
            panic!("add failed: {:?}", res.error);
        };
        assert_eq!(change, SaveChange::Created);
        assert!(!forward.has_stored_password);
        assert_eq!(state.configs_snapshot().len(), 1);

        // A second forward may not reuse the name (names are CLI targets).
        let dup = handle_request(&state, with_fields(CliCommand::Add, None, fields("DB"))).await;
        assert!(matches!(dup.error, Some(AppError::InvalidInput(_))));

        let patch = ForwardFields {
            local_port: Some(25432),
            ..ForwardFields::default()
        };
        let res = handle_request(&state, with_fields(CliCommand::Edit, Some("db"), patch)).await;
        let Some(CliData::Saved {
            forward: edited,
            change,
        }) = res.data
        else {
            panic!("edit failed: {:?}", res.error);
        };
        assert_eq!(change, SaveChange::Updated);
        assert_eq!(edited.id, forward.id, "edit keeps the id");
        assert_eq!(edited.local_port, 25432);
        assert_eq!(edited.remote_port, 5432, "untouched field kept");

        let empty = handle_request(
            &state,
            with_fields(CliCommand::Edit, Some("db"), ForwardFields::default()),
        )
        .await;
        assert!(matches!(empty.error, Some(AppError::InvalidInput(_))));

        let res = handle_request(&state, target_req(CliCommand::Remove, "db")).await;
        assert!(res.ok, "remove failed: {:?}", res.error);
        assert!(state.configs_snapshot().is_empty());
    }

    #[tokio::test]
    async fn password_set_flips_only_the_flag_and_clear_resets_it() {
        let state = state_with(&[("id-a", "prod-db")]);
        state
            .set_password_checked("id-a", "hunter2".into())
            .await
            .expect("seed secret");
        // sample_config starts with the flag on; clear it first.
        let res = handle_request(&state, target_req(CliCommand::PasswordClear, "prod-db")).await;
        assert!(res.ok, "{:?}", res.error);
        assert!(!state.get_config("id-a").expect("cfg").has_stored_password);
        assert_eq!(state.get_password("id-a"), None, "clear deletes the secret");

        let res = handle_request(&state, target_req(CliCommand::PasswordSet, "prod-db")).await;
        let Some(CliData::Saved { forward, change }) = res.data else {
            panic!("password-set failed: {:?}", res.error);
        };
        assert_eq!(change, SaveChange::PasswordSet);
        assert!(forward.has_stored_password);
    }
}
