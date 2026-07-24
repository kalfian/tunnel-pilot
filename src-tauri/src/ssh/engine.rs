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
use std::sync::atomic::Ordering;
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
    /// Session drop / dead-channel / failed wake probe → reconnect path.
    Reconnect,
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
    state.registry.insert(handle);
    let _ = go_tx.send(());
    Ok(())
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
                match handle_teardown(
                    &state,
                    &id,
                    &parent,
                    &retry_notify,
                    &settings,
                    &mut reconnect_attempt,
                    e.to_string(),
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
            let _ = session
                .disconnect(russh::Disconnect::ByApplication, "", "")
                .await;
            break 'supervisor;
        }
        state.emit_status(&id, ForwardStatus::Connected, None);
        let _ = stats_tx.send(stats.snapshot());
        state.emit_stats(&id, stats.snapshot());

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
        let _ = session
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await;
        drop(session);

        match disposition {
            Disposition::UserExit => break 'supervisor,
            Disposition::Reconnect => {
                match handle_teardown(
                    &state,
                    &id,
                    &parent,
                    &retry_notify,
                    &settings,
                    &mut reconnect_attempt,
                    "connection lost".to_string(),
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

            // Wake-from-sleep nudge (NIT-1/§4): immediate RTT probe; if it fails,
            // reconnect now (bypassing backoff via the reconnect path).
            _ = ctx.wake_notify.notified() => {
                if !run_rtt_probe(ctx.session, ctx.stats).await {
                    tracing::info!(tunnel = %ctx.id, "wake probe failed -> reconnect");
                    return Disposition::Reconnect;
                }
            }

            // Stats + latency cadence (§6): probe RTT on the OWNED session,
            // publish a snapshot into the cell, and emit tunnel://stats. A failed
            // probe leaves latency unchanged and is NOT a teardown signal.
            _ = probe.tick() => {
                run_rtt_probe(ctx.session, ctx.stats).await;
                let snap = ctx.stats.snapshot();
                let _ = ctx.stats_tx.send(snap.clone());
                ctx.state.emit_stats(ctx.id, snap);
            }
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
async fn handle_teardown(
    state: &Arc<AppState>,
    id: &str,
    parent: &CancellationToken,
    retry_notify: &Notify,
    settings: &crate::state::models::AppSettings,
    reconnect_attempt: &mut u32,
    msg: String,
) -> Flow {
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
            state.emit_status(id, ForwardStatus::Error, Some(msg));
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
