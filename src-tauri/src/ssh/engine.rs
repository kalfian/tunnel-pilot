//! Long-lived per-tunnel supervisor + the connect/disconnect/retry entry points
//! (spec 03 §§1,3,5).
//!
//! One supervisor task per tunnel owns its russh session IN-TASK and loops
//! across reconnect attempts; its `JoinHandle` never changes (F21). The session
//! handle is NEVER stored in the registry. Cancellation is two-level (F6):
//! a durable `parent_cancel` (cancelled only by disconnect/delete) and a
//! per-attempt `attempt_cancel = parent.child_token()` re-minted each attempt.
//!
//! Status is written ONLY via the guarded `registry.set_status` (F23/F28): the
//! supervisor writes connecting/connected/error; the command handler
//! (`disconnect_forward`) writes disconnecting/disconnected.

use std::io::ErrorKind;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::net::TcpListener;
use tokio::sync::{oneshot, watch, Notify};
use tokio::time::{interval, timeout, MissedTickBehavior};
use tokio_util::sync::CancellationToken;

use crate::error::AppError;
use crate::ssh::client::{self, Session};
use crate::ssh::forward::{spawn_forward_conn, ForwardFailSignal, MAX_FORWARD_FAILURES};
use crate::ssh::reconnect::backoff;
use crate::ssh::stats::StatsInner;
use crate::state::models::{ForwardConfig, ForwardStatus, TunnelStats};
use crate::state::tunnel_registry::TunnelHandle;
use crate::state::AppState;

/// Local-port `EADDRINUSE` bind-retry parameters (spec 03 §1, F25 — subsumes
/// v1's 15×200ms port-release poll).
const BIND_RETRIES: u32 = 5;
const BIND_RETRY_DELAY: Duration = Duration::from_millis(500);
/// Liveness poll cadence — checks `Handle::is_closed()` (F7/F16-spike). Cheap;
/// the real teardown authority is russh keepalive.
const LIVENESS_POLL: Duration = Duration::from_secs(1);
/// Stats + latency probe cadence (spec 03 §§2,6).
const STATS_PROBE: Duration = Duration::from_secs(3);
/// RTT probe timeout.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Upper bound on the best-effort SSH-disconnect during teardown (F32). Sending
/// `Msg::Disconnect` only queues on the session's channel, but a wedged session
/// must never let teardown stall past cancel latency.
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(1);

/// What the supervisor should do after a session teardown.
enum Flow {
    /// User disconnect/delete cancelled the parent → exit the supervisor.
    Exit,
    /// Non-user teardown, eligible/parked → loop to the next attempt.
    Reconnect,
}

/// What ended the accept loop.
enum Disposition {
    /// Parent token cancelled → user teardown.
    UserExit,
    /// Session drop / dead-channel → reconnect via the normal backoff path.
    Reconnect,
    /// A wake probe found the session dead (§4) → reconnect NOW, skipping the
    /// backoff wait (the machine just resumed; recover immediately).
    ReconnectImmediate,
}

/// Start (or restart) a tunnel: launch its supervisor task (spec 03 §1).
///
/// Conflict handling first (spec 03 §1): if this id is already live, disconnect
/// it; then disconnect any OTHER tunnel bound to the same local `(addr, port)`
/// before binding.
pub async fn connect_forward(state: &Arc<AppState>, id: &str) -> Result<(), AppError> {
    let cfg = state
        .get_config(id)
        .ok_or_else(|| AppError::NotFound(format!("forward {id}")))?;

    // Conflict: already-connected same id → disconnect first.
    if state.registry.contains(id) {
        disconnect_forward(state, id, false).await?;
    }
    // Conflict: any OTHER tunnel on the same local port → disconnect it.
    for other in state.registry.all_ids() {
        if other == id {
            continue;
        }
        if let Some(oc) = state.get_config(&other) {
            if oc.local_bind_address == cfg.local_bind_address && oc.local_port == cfg.local_port {
                disconnect_forward(state, &other, false).await?;
            }
        }
    }

    // Atomically reserve the id (F33): if a concurrent connect already reserved
    // it or it is already live, bail — this guarantees at most ONE supervisor
    // per tunnel even when multiple drivers race (M4 drives this concurrently).
    // A second connect losing the race is a benign no-op; the winner is live.
    if !state.registry.try_begin_start(id) {
        return Ok(());
    }
    // F36: RAII-release the reservation if this fn panics/returns before the
    // handle is inserted. Disarmed on the success path (insert clears it).
    let reservation = StartReservation {
        registry: state.registry.clone(),
        id: id.to_string(),
        armed: true,
    };

    // Shared per-tunnel state. The status starts Disconnected; the supervisor's
    // first guarded write moves it to Connecting (a legal transition).
    let parent_cancel = CancellationToken::new();
    let attempt_cancel = parent_cancel.child_token();
    let (status_tx, _status_rx) = watch::channel(ForwardStatus::Disconnected);
    let (stats_tx, stats_rx) = watch::channel(TunnelStats::default());
    let stats = Arc::new(StatsInner::default());
    let retry_notify = Arc::new(Notify::new());
    let wake_notify = Arc::new(Notify::new());

    // go-gate: the supervisor waits until the registry entry is inserted, so its
    // first `set_status`/token lookup never races the insert.
    let (go_tx, go_rx) = oneshot::channel::<()>();
    let join = tokio::spawn(supervise(state.clone(), id.to_string(), stats_tx, go_rx));

    let handle = TunnelHandle {
        id: id.to_string(),
        parent_cancel,
        attempt_cancel,
        join,
        status: status_tx,
        last_error: None,
        retry_requested: false,
        retry_notify,
        wake_notify,
        stats_cell: stats_rx,
        stats,
    };
    state.registry.insert(handle); // clears the "starting" reservation
    reservation.disarm(); // insert already released it; don't double-release
    let _ = go_tx.send(());
    Ok(())
}

/// RAII release of a [`TunnelRegistry::try_begin_start`] reservation (F36). If
/// `connect_forward` unwinds or returns between the reserve and the `insert`,
/// the drop clears the "starting" flag so the id can be started again — the
/// reservation can never leak on a panic path. Disarmed once `insert` (which
/// clears the reservation itself) has run on the success path.
struct StartReservation {
    registry: Arc<crate::state::tunnel_registry::TunnelRegistry>,
    id: String,
    armed: bool,
}

impl StartReservation {
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for StartReservation {
    fn drop(&mut self) {
        if self.armed {
            self.registry.finish_start(&self.id);
        }
    }
}

/// User (or conflict) disconnect: cancel the parent, await the supervisor's
/// stable `JoinHandle`, then reach `disconnected` (spec 03 §1, F21/F31).
///
/// Ownership (F23): the command handler is the sole writer of
/// disconnecting/disconnected. Clicking while already `disconnecting` is a
/// no-op (v1 toggle semantics).
pub async fn disconnect_forward(
    state: &Arc<AppState>,
    id: &str,
    user_initiated: bool,
) -> Result<(), AppError> {
    let Some(current) = state.registry.current_status(id) else {
        // Not live — nothing to do (idempotent).
        return Ok(());
    };
    if current == ForwardStatus::Disconnecting {
        // Ignore clicks during the transient disconnecting state (F23).
        return Ok(());
    }

    // connected/connecting/error → disconnecting (guarded, F31 allows connecting).
    let out = state
        .registry
        .set_status(id, ForwardStatus::Disconnecting, None);
    if out.applied {
        state.emit_status(id, ForwardStatus::Disconnecting, None);
    }

    // Cancel the durable parent → supervisor loop (connecting/backoff/accept/
    // parked-error) ends; its listener + channels + session are dropped in-task
    // (F24 releases the bound port within cancel latency).
    state.registry.cancel_parent(id);

    // Take the handle out and await the supervisor so cleanup is deterministic
    // BEFORE the entry disappears (F21) — the port is released before removal.
    if let Some(handle) = state.registry.remove(id) {
        let _ = handle.join.await;
        handle.stats.mark_disconnected();
    }

    // Reached disconnected (F31): emit so the UI is never stranded.
    tracing::info!(tunnel = %id, user_initiated, "disconnected");
    state.emit_status(id, ForwardStatus::Disconnected, None);
    Ok(())
}

/// Retry a tunnel parked in `error` (spec 03 §1, F27c/F29). Acts ONLY when
/// status==error: sets the lock-guarded flag + mints a fresh attempt token in
/// one section, then pokes the wakeup. Reuses the same supervisor/JoinHandle.
pub async fn retry_forward(state: &Arc<AppState>, id: &str) -> Result<(), AppError> {
    if let Some(notify) = state.registry.request_retry(id) {
        notify.notify_one(); // wakeup only; the flag is the truth (F29)
        tracing::info!(tunnel = %id, "retry requested");
    }
    // If not parked in error, request_retry returned None → silent no-op (F27c).
    Ok(())
}

/// Nudge a tunnel's supervisor to probe its session immediately (wake-from-sleep
/// path, §4). Wired to the wake watchdog in M2; here it feeds the supervisor's
/// wake-probe `select!` arm.
pub fn request_wake_probe(state: &AppState, id: &str) {
    if let Some(notify) = state.registry.wake_notify(id) {
        notify.notify_one();
    }
}

/// The single long-lived supervisor task (F21).
async fn supervise(
    state: Arc<AppState>,
    id: String,
    stats_tx: watch::Sender<TunnelStats>,
    go_rx: oneshot::Receiver<()>,
) {
    // Wait until the registry entry exists (go-gate).
    let _ = go_rx.await;

    let cfg = match state.get_config(&id) {
        Some(c) => Arc::new(c),
        None => {
            let out = state.registry.set_status(
                &id,
                ForwardStatus::Error,
                Some("config not found".into()),
            );
            if out.applied {
                state.emit_status(&id, ForwardStatus::Error, Some("config not found".into()));
            }
            return;
        }
    };

    let Some(parent) = state.registry.parent_token(&id) else {
        return;
    };
    let Some(retry_notify) = state.registry.retry_notify(&id) else {
        return;
    };
    let Some(wake_notify) = state.registry.wake_notify(&id) else {
        return;
    };
    let Some(stats) = state.registry.stats(&id) else {
        return;
    };

    let mut reconnect_attempt: u32 = 0;

    'supervisor: loop {
        // ---- per-attempt reset (F27a/F30) ----
        // Clear any stale retry flag so a retry from a PRIOR park/backoff cycle
        // cannot un-park a FUTURE error (F27c). A genuine parked-retry was
        // already consumed via take_retry_requested before we got here.
        state.registry.set_retry_requested(&id, false);
        let attempt_cancel = match state.registry.mint_fresh_attempt(&id) {
            Some(t) => t,
            None => return, // entry gone → nothing to supervise
        };
        // Fresh per-attempt failure signal (counter + notify) — F27a/F30.
        let fail = ForwardFailSignal::new();

        // ---- status → connecting ----
        let out = state
            .registry
            .set_status(&id, ForwardStatus::Connecting, None);
        if out.applied {
            state.emit_status(&id, ForwardStatus::Connecting, None);
        } else {
            // Could not enter connecting (a user disconnect set disconnecting) →
            // the parent will be/was cancelled; stop supervising.
            break 'supervisor;
        }

        let settings = state.settings_snapshot();

        // ---- cancellation-aware bind → connect → auth (F24) ----
        // A disconnect during this phase cancels the attempt (child of parent),
        // run_until_cancelled returns None, and the bound listener + in-flight
        // connect future are dropped immediately (port released fast).
        let connect_result = attempt_cancel
            .run_until_cancelled(bind_connect_auth(&state, &cfg))
            .await;

        let (listener, session) = match connect_result {
            None => {
                if parent.is_cancelled() {
                    break 'supervisor; // user disconnect/delete
                } else {
                    continue 'supervisor; // attempt reset (fresh token minted)
                }
            }
            Some(Ok(pair)) => pair,
            Some(Err(e)) => {
                // Surface the EXACT russh failure reason (KEX/host-key/cipher
                // negotiation, bind, or auth) — otherwise only the later
                // "disconnected" line is logged and the real cause is hidden.
                tracing::warn!(tunnel = %id, error = %e, "connect attempt failed");
                match handle_teardown(
                    TeardownCtx {
                        state: &state,
                        id: &id,
                        parent: &parent,
                        retry_notify: &retry_notify,
                        settings: &settings,
                    },
                    &mut reconnect_attempt,
                    e.to_string(),
                    false,
                )
                .await
                {
                    Flow::Exit => break 'supervisor,
                    Flow::Reconnect => continue 'supervisor,
                }
            }
        };

        // ---- CONNECTED ----
        reconnect_attempt = 0; // success resets backoff
        stats.mark_connected();
        let out = state
            .registry
            .set_status(&id, ForwardStatus::Connected, None);
        if !out.applied {
            // Lost the race to a user disconnect (now disconnecting) → tear this
            // session down and let the command handler finish to disconnected.
            drop(listener);
            let _ = timeout(
                DISCONNECT_TIMEOUT,
                session.disconnect(russh::Disconnect::ByApplication, "", ""),
            )
            .await;
            break 'supervisor;
        }
        state.emit_status(&id, ForwardStatus::Connected, None);
        // Notify on connect (user connect OR auto-reconnect success), matching
        // v1 `showConnected`; honors `showNotifications` + best-effort on
        // unsigned macOS (spec 03 §15).
        crate::platform::notify::notify_connected(&state, &cfg.name);
        // Publish the fresh snapshot into the cell; the shared sampler
        // (health.rs) is the SOLE emitter of `tunnel://stats` (spec 03 §2), so
        // the supervisor does NOT emit here — it only feeds the cell.
        let _ = stats_tx.send(stats.snapshot());
        // Auto-start the single shared stats sampler on (re)connect; idempotent
        // while one is already running (spec 03 §2).
        crate::ssh::health::ensure_sampler(&state);

        let session = Arc::new(session);

        // ---- accept loop ----
        let disposition = accept_loop(AcceptCtx {
            state: &state,
            id: &id,
            cfg: &cfg,
            session: &session,
            listener: &listener,
            stats: &stats,
            stats_tx: &stats_tx,
            fail: &fail,
            attempt_cancel: &attempt_cancel,
            parent: &parent,
            wake_notify: &wake_notify,
        })
        .await;

        // ---- teardown of this session ----
        stats.mark_disconnected();
        drop(listener); // release the local port
        let _ = timeout(
            DISCONNECT_TIMEOUT,
            session.disconnect(russh::Disconnect::ByApplication, "", ""),
        )
        .await;
        drop(session);

        match disposition {
            Disposition::UserExit => break 'supervisor,
            Disposition::Reconnect => {
                match handle_teardown(
                    TeardownCtx {
                        state: &state,
                        id: &id,
                        parent: &parent,
                        retry_notify: &retry_notify,
                        settings: &settings,
                    },
                    &mut reconnect_attempt,
                    "connection lost".to_string(),
                    false,
                )
                .await
                {
                    Flow::Exit => break 'supervisor,
                    Flow::Reconnect => continue 'supervisor,
                }
            }
            // Wake probe found the session dead (§4): recover NOW, skipping the
            // backoff wait (`immediate = true`). Still routed through
            // `handle_teardown` so it sets `error` first (the legal pre-reconnect
            // transition), honors `auto_reconnect=false` (park in error, not a
            // silent stall), and yields to a concurrent user disconnect.
            Disposition::ReconnectImmediate => {
                match handle_teardown(
                    TeardownCtx {
                        state: &state,
                        id: &id,
                        parent: &parent,
                        retry_notify: &retry_notify,
                        settings: &settings,
                    },
                    &mut reconnect_attempt,
                    "connection lost (wake probe)".to_string(),
                    true,
                )
                .await
                {
                    Flow::Exit => break 'supervisor,
                    Flow::Reconnect => continue 'supervisor,
                }
            }
        }
    }

    tracing::debug!(tunnel = %id, "supervisor task exiting");
}

/// Bind the local listener (5×500ms EADDRINUSE retry), SSH-connect, and
/// authenticate. Runs entirely inside `run_until_cancelled` so a teardown during
/// any step drops the (possibly bound) listener and aborts the connect (F24).
async fn bind_connect_auth(
    state: &Arc<AppState>,
    cfg: &ForwardConfig,
) -> Result<(TcpListener, Session), AppError> {
    let addr = format!("{}:{}", cfg.local_bind_address, cfg.local_port);
    let listener = bind_local(&addr).await?;
    let mut session = client::connect(cfg).await?;
    client::authenticate(&mut session, cfg, state).await?;
    Ok((listener, session))
}

/// Bind with 5×500ms retry on `AddrInUse` (spec 03 §1, F25). `ErrorKind` is
/// portable across the three OSes (no hardcoded errno).
async fn bind_local(addr: &str) -> Result<TcpListener, AppError> {
    let mut last: Option<std::io::Error> = None;
    for attempt in 0..BIND_RETRIES {
        match TcpListener::bind(addr).await {
            Ok(l) => return Ok(l),
            Err(e) if e.kind() == ErrorKind::AddrInUse => {
                last = Some(e);
                if attempt + 1 < BIND_RETRIES {
                    tokio::time::sleep(BIND_RETRY_DELAY).await;
                }
            }
            Err(e) => return Err(AppError::Connection(format!("bind {addr} failed: {e}"))),
        }
    }
    Err(AppError::Connection(format!(
        "local port {addr} in use after {BIND_RETRIES} attempts: {}",
        last.map(|e| e.to_string()).unwrap_or_default()
    )))
}

/// Parameters for the accept loop (grouped to keep the signature readable).
struct AcceptCtx<'a> {
    state: &'a Arc<AppState>,
    id: &'a str,
    cfg: &'a Arc<ForwardConfig>,
    session: &'a Arc<Session>,
    listener: &'a TcpListener,
    stats: &'a Arc<StatsInner>,
    stats_tx: &'a watch::Sender<TunnelStats>,
    fail: &'a ForwardFailSignal,
    attempt_cancel: &'a CancellationToken,
    parent: &'a CancellationToken,
    wake_notify: &'a Notify,
}

/// The steady-state accept loop (spec 03 §1). Five `select!` arms:
/// attempt-cancel, accept, session-lost poll (F7), dead-channel wake (F26), and
/// the wake-probe nudge (NIT-1) — plus the stats/RTT probe tick (§6).
async fn accept_loop(ctx: AcceptCtx<'_>) -> Disposition {
    let mut liveness = interval(LIVENESS_POLL);
    liveness.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut probe = interval(STATS_PROBE);
    probe.set_missed_tick_behavior(MissedTickBehavior::Skip);

    // F32: the RTT probe can hang up to PROBE_TIMEOUT (3s) on a WEDGED session
    // (TCP up, SSH stalled). It MUST NOT run inline in this `select!` — that
    // would block user-disconnect (`attempt_cancel`), `accept`, and the cheap
    // `is_closed()` liveness poll for up to 3s. So probes are SPAWNED off the
    // loop: the stats probe updates latency in the shared cell for the next
    // publish tick, and a wake probe signals `wake_dead` on failure.
    let probe_in_flight = Arc::new(AtomicBool::new(false));
    let wake_dead = Arc::new(Notify::new());

    loop {
        tokio::select! {
            // Attempt token cancelled — disambiguate (F27d): parent cancelled =
            // user teardown → exit; otherwise attempt reset → reconnect path.
            _ = ctx.attempt_cancel.cancelled() => {
                if ctx.parent.is_cancelled() {
                    return Disposition::UserExit;
                }
                return Disposition::Reconnect;
            }

            // New local connection → spawn a forward child.
            accept = ctx.listener.accept() => {
                match accept {
                    Ok((sock, _peer)) => {
                        spawn_forward_conn(
                            ctx.session.clone(),
                            sock,
                            ctx.cfg.clone(),
                            ctx.stats.clone(),
                            ctx.fail.clone(),
                            ctx.attempt_cancel.clone(),
                        );
                    }
                    Err(e) => {
                        tracing::warn!(tunnel = %ctx.id, error = %e, "accept error");
                    }
                }
            }

            // Session-lost poll (F7): the private session future exited (keepalive
            // timeout / hard drop) → is_closed() flips true → reconnect path.
            _ = liveness.tick() => {
                if ctx.session.is_closed() {
                    tracing::info!(tunnel = %ctx.id, "ssh session closed (keepalive/drop) -> reconnect");
                    return Disposition::Reconnect;
                }
            }

            // Dead-channel WAKE (F26/F27b): re-check the AUTHORITATIVE per-attempt
            // counter; only tear down at >= 3, else keep serving (spurious wake).
            _ = ctx.fail.notify.notified() => {
                if ctx.fail.count.load(Ordering::SeqCst) >= MAX_FORWARD_FAILURES {
                    tracing::info!(tunnel = %ctx.id, "3 consecutive forward failures -> reconnect");
                    return Disposition::Reconnect;
                }
            }

            // A SPAWNED wake probe found the session dead → reconnect fast,
            // bypassing the backoff wait (§4). Non-blocking: the probe ran off
            // the loop.
            _ = wake_dead.notified() => {
                tracing::info!(tunnel = %ctx.id, "wake probe failed -> immediate reconnect");
                return Disposition::ReconnectImmediate;
            }

            // Wake-from-sleep nudge (NIT-1/§4): SPAWN an immediate RTT probe that
            // pokes `wake_dead` on failure. Spawned (not inline) so a wedged
            // probe can't block the loop (F32). Wake events are rare (sleep
            // resume), so this probe is not gated by `probe_in_flight`.
            _ = ctx.wake_notify.notified() => {
                spawn_rtt_probe(ctx.session.clone(), ctx.stats.clone(), None, Some(wake_dead.clone()));
            }

            // Stats + latency cadence (§6): publish the current snapshot (cheap,
            // non-blocking) and SPAWN a latency probe whose result lands in the
            // cell for the NEXT publish. A failed/wedged probe leaves latency
            // unchanged, never tears down, and never blocks this loop (F32). The
            // `probe_in_flight` guard prevents 3s-cadence pile-up on a wedged
            // session (one probe at a time).
            _ = probe.tick() => {
                // Publish the current snapshot into the cell for the shared
                // sampler to emit (spec 03 §2 — supervisor feeds the cell, never
                // emits). Then SPAWN a latency probe whose result lands in the
                // cell for the NEXT tick (F32: off-loop, never blocks here).
                let _ = ctx.stats_tx.send(ctx.stats.snapshot());
                spawn_rtt_probe(ctx.session.clone(), ctx.stats.clone(), Some(probe_in_flight.clone()), None);
            }
        }
    }
}

/// Spawn an RTT probe OFF the accept loop (F32). Updates `stats` latency on
/// success; pokes `dead` (if any) on failure. `in_flight`, when provided, caps
/// concurrency to one probe (used by the periodic stats probe to avoid pile-up
/// on a wedged session); the wake probe passes `None` so it always runs.
fn spawn_rtt_probe(
    session: Arc<Session>,
    stats: Arc<StatsInner>,
    in_flight: Option<Arc<std::sync::atomic::AtomicBool>>,
    dead: Option<Arc<Notify>>,
) {
    if let Some(flag) = &in_flight {
        if flag.swap(true, Ordering::SeqCst) {
            return; // a probe is already running
        }
    }
    tokio::spawn(async move {
        // F36: RAII-clear the in-flight flag on drop so a panic in the probe
        // can't leak it (which would wedge the pile-up guard forever). Cleared
        // on the normal path too, when the guard drops at end of scope.
        let _guard = InFlightGuard(in_flight);
        let ok = run_rtt_probe(&session, &stats).await;
        if !ok {
            if let Some(d) = dead {
                d.notify_one();
            }
        }
    });
}

/// RAII clear of the periodic-probe in-flight flag (F36). Holds the flag that
/// `spawn_rtt_probe` already `swap(true)`'d; the drop stores `false` — on the
/// normal path at end of scope, and during unwind if the probe panics.
struct InFlightGuard(Option<Arc<AtomicBool>>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if let Some(flag) = &self.0 {
            flag.store(false, Ordering::SeqCst);
        }
    }
}

/// Channel-open RTT probe on the OWNED session (spec 03 §6). There is no
/// `ping()`; timing `channel_open_session` + close is the latency measure.
/// Returns whether the probe succeeded (a failure does NOT tear down).
async fn run_rtt_probe(session: &Session, stats: &StatsInner) -> bool {
    let t0 = Instant::now();
    let result = timeout(PROBE_TIMEOUT, async {
        let ch = session.channel_open_session().await?;
        ch.close().await
    })
    .await;
    match result {
        Ok(Ok(())) => {
            stats.set_latency(t0.elapsed());
            true
        }
        _ => false,
    }
}

/// Set `error` and decide reconnect-vs-park (spec 03 §§1,3). Eligible
/// (`auto_reconnect && attempt < max`) → backoff then reconnect. Otherwise PARK:
/// set error + check-and-clear the retry flag in the SAME critical section
/// (F29), then `select!` parent-cancel vs the retry wakeup (re-checking the flag
/// on wake — the load-bearing check). Returns whether to exit or reconnect.
///
/// `immediate` (wake-probe recovery, §4): when eligible, still enter `error`
/// (the legal pre-`connecting` transition) but SKIP the backoff wait and do not
/// consume a retry-budget slot — the machine just resumed, so reconnect at once.
/// A subsequent failure on that fresh attempt falls back to the normal backoff.
/// The stable per-tunnel environment for a teardown decision (grouped to keep
/// the signature within clippy's arg budget, mirroring `AcceptCtx`).
struct TeardownCtx<'a> {
    state: &'a Arc<AppState>,
    id: &'a str,
    parent: &'a CancellationToken,
    retry_notify: &'a Notify,
    settings: &'a crate::state::models::AppSettings,
}

async fn handle_teardown(
    ctx: TeardownCtx<'_>,
    reconnect_attempt: &mut u32,
    msg: String,
    immediate: bool,
) -> Flow {
    let TeardownCtx {
        state,
        id,
        parent,
        retry_notify,
        settings,
    } = ctx;

    // A concurrent user disconnect already cancelled the parent → just exit; the
    // command handler owns the disconnecting/disconnected transitions (never
    // flash disconnecting -> error, F28).
    if parent.is_cancelled() {
        return Flow::Exit;
    }

    let eligible =
        settings.auto_reconnect && *reconnect_attempt < settings.auto_reconnect_max_retries;

    if eligible {
        // Transient error between attempts; status stays error only until the
        // next attempt sets connecting. NOTE: status IS `error` during backoff,
        // but a retry fired now is absorbed — the top-of-attempt flag clear means
        // it cannot un-park a FUTURE error (F27c).
        let out = state
            .registry
            .set_status(id, ForwardStatus::Error, Some(msg.clone()));
        if out.applied {
            state.emit_status(id, ForwardStatus::Error, Some(msg));
        }
        if immediate {
            // Wake recovery (§4): the `error` above is the legal pre-`connecting`
            // transition; loop straight into a fresh attempt with no backoff wait
            // and no budget consumed. A concurrent user disconnect still wins on
            // the next attempt's cancellation-aware connect.
            return Flow::Reconnect;
        }
        let delay = backoff(settings.auto_reconnect_delay_sec, *reconnect_attempt);
        *reconnect_attempt += 1;
        tokio::select! {
            _ = parent.cancelled() => Flow::Exit,
            _ = tokio::time::sleep(delay) => Flow::Reconnect,
        }
    } else {
        // PARK. begin_terminal_error sets error + check-and-clears retry in ONE
        // section (F29 defensive in-section check).
        let term = state.registry.begin_terminal_error(id, Some(msg.clone()));
        if term.applied {
            state.emit_status(id, ForwardStatus::Error, Some(msg.clone()));
            // Terminal error (retries exhausted / auto-reconnect off) — notify,
            // matching v1's "notify only when no retry was scheduled" (spec 03
            // §15). The transient-error branch above deliberately does NOT
            // notify. Resolve the display name from the config (linear scan; not
            // a hot path), falling back to the id.
            let name = state
                .get_config(id)
                .map(|c| c.name)
                .unwrap_or_else(|| id.to_string());
            crate::platform::notify::notify_error(state, &name, &msg);
        }
        if term.retry_already_requested {
            *reconnect_attempt = 0;
            return Flow::Reconnect;
        }
        loop {
            tokio::select! {
                _ = parent.cancelled() => return Flow::Exit,
                _ = retry_notify.notified() => {
                    // Load-bearing re-check-and-clear under the lock (F29).
                    if state.registry.take_retry_requested(id) {
                        *reconnect_attempt = 0;
                        return Flow::Reconnect;
                    }
                    // spurious wake (no real request) → keep parking
                }
            }
        }
    }
}
