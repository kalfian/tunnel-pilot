//! Request handlers for the control socket — the only place the CLI touches
//! app state (spec 03 §20).
//!
//! Every mutation goes through the SAME service functions the tray and the
//! webview use (`ssh::engine::*`, `commands::forwards::run_start_all` /
//! `run_stop_all`), so a CLI-driven connect emits `tunnel://status`, refreshes
//! the tray, and is visible to the frontend on its next hydrate. The CLI is a
//! second *driver*, never a second state owner.
//!
//! Nothing here reads or writes a credential: there is no password command and
//! [`ForwardView`] carries only the `hasStoredPassword` flag (AGENTS §8).

use std::sync::Arc;

use tokio::sync::watch;
use tokio::time::{timeout_at, Duration, Instant};

use crate::cli::protocol::{
    BulkResult, CliCommand, CliData, CliRequest, CliResponse, ForwardView, PROTOCOL_VERSION,
};
use crate::commands::forwards::{run_start_all, run_stop_all, runtime_or_default};
use crate::error::AppError;
use crate::ssh::engine;
use crate::state::models::{ForwardConfig, ForwardStatus};
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
}
