//! Single shared 3s stats/latency EMIT sampler (reads each tunnel's stats
//! cell, emits `tunnel://stats`). Holds NO session and NEVER tears down —
//! liveness is owned by russh keepalive + the session-future signal (F1/F21).
//!
//! Ownership split (spec 03 §2): the per-tunnel SUPERVISOR owns the live session
//! and publishes a fresh `TunnelStats` snapshot (including the channel-open RTT
//! latency) into its `stats_cell` (`watch`) every 3s. This single shared task
//! is the SOLE emitter of `tunnel://stats` — it reads every connected tunnel's
//! cell on its own 3s tick and emits, touching no session. Exactly one sampler
//! exists regardless of tunnel count: it auto-starts on the first connect
//! (`ensure_sampler`, idempotent via `registry.try_start_sampler`) and
//! auto-stops when no connected tunnels remain.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::{interval, MissedTickBehavior};

use crate::state::models::{TunnelId, TunnelStats};
use crate::state::AppState;

/// Emit cadence (spec 03 §§2,6). Matches the supervisor's cell-publish cadence.
const SAMPLE_INTERVAL: Duration = Duration::from_secs(3);

/// Start the shared sampler iff one is not already running (idempotent). Called
/// each time a tunnel reaches `connected` (spec 03 §2 "auto-start on first
/// connect"); subsequent calls while a sampler is live are no-ops.
pub fn ensure_sampler(state: &Arc<AppState>) {
    if state.registry.try_start_sampler() {
        let state = state.clone();
        tokio::spawn(sampler_loop(state));
    }
}

/// One sampler pass: the (id, stats) pairs to emit this tick. A pure READ of the
/// connected tunnels' stats cells — no session access, no teardown, no mutation.
/// Extracted so the sample-and-never-teardown invariant is unit-testable.
fn sample_once(state: &AppState) -> Vec<(TunnelId, TunnelStats)> {
    state
        .registry
        .connected_snapshot()
        .into_iter()
        .map(|(id, cell)| (id, cell.borrow().clone()))
        .collect()
}

/// The single shared emit loop. Reads cells and emits; NEVER tears a session
/// down. Auto-stops when no connected tunnels remain, re-checking after
/// releasing the slot to close the race with a concurrent [`ensure_sampler`].
async fn sampler_loop(state: Arc<AppState>) {
    let mut tick = interval(SAMPLE_INTERVAL);
    // If a tick is delayed (busy runtime / resume), skip the missed ones rather
    // than bursting — this is an emit cadence, not a deadline.
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tracing::debug!("stats sampler started");

    loop {
        tick.tick().await;

        let batch = sample_once(&state);
        if batch.is_empty() {
            // Attempt to stop. Release the slot FIRST, then re-check: a tunnel
            // that reached `connected` between the snapshot and the release must
            // not be left without a sampler (auto-restart race).
            state.registry.stop_sampler();
            if sample_once(&state).is_empty() {
                break; // truly none connected → auto-stop
            }
            // A tunnel connected in the gap. Re-claim the slot; if another
            // `ensure_sampler` already re-claimed it, yield ownership and exit.
            if !state.registry.try_start_sampler() {
                break;
            }
            continue;
        }

        for (id, stats) in batch {
            state.emit_stats(&id, stats);
        }
    }

    tracing::debug!("stats sampler stopped (no connected tunnels)");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::stats::StatsInner;
    use crate::state::models::ForwardStatus;
    use crate::state::tunnel_registry::TunnelHandle;
    use std::sync::Arc;
    use tokio::sync::{watch, Notify};
    use tokio_util::sync::CancellationToken;

    /// Build a live handle whose stats cell carries `stats`, at `status`.
    fn seed_handle(id: &str, status: ForwardStatus, stats: TunnelStats) -> TunnelHandle {
        let parent = CancellationToken::new();
        let attempt = parent.child_token();
        let (status_tx, _rx) = watch::channel(status);
        let (_stats_tx, stats_rx) = watch::channel(stats);
        TunnelHandle {
            id: id.to_string(),
            parent_cancel: parent,
            attempt_cancel: attempt,
            join: tokio::spawn(async {}),
            status: status_tx,
            last_error: None,
            retry_requested: false,
            retry_notify: Arc::new(Notify::new()),
            wake_notify: Arc::new(Notify::new()),
            stats_cell: stats_rx,
            stats: Arc::new(StatsInner::default()),
        }
    }

    fn stats_with_latency(latency_ms: u64) -> TunnelStats {
        TunnelStats {
            active_connections: 2,
            total_bytes_up: 100,
            total_bytes_down: 200,
            last_ping_latency_ms: Some(latency_ms),
            connected_since: Some("2026-01-01T00:00:00Z".to_string()),
        }
    }

    #[tokio::test]
    async fn sample_once_reads_stats_and_latency_from_connected_cells() {
        let state = Arc::new(AppState::new_headless());
        state.registry.insert(seed_handle(
            "a",
            ForwardStatus::Connected,
            stats_with_latency(42),
        ));

        let batch = sample_once(&state);
        assert_eq!(batch.len(), 1, "one connected tunnel sampled");
        assert_eq!(batch[0].0, "a");
        assert_eq!(
            batch[0].1.last_ping_latency_ms,
            Some(42),
            "sampler carries the RTT latency from the cell"
        );
        assert_eq!(batch[0].1.active_connections, 2);
    }

    #[tokio::test]
    async fn sample_once_skips_non_connected_and_never_mutates_status() {
        let state = Arc::new(AppState::new_headless());
        state.registry.insert(seed_handle(
            "up",
            ForwardStatus::Connected,
            stats_with_latency(5),
        ));
        state.registry.insert(seed_handle(
            "err",
            ForwardStatus::Error,
            TunnelStats::default(),
        ));
        state.registry.insert(seed_handle(
            "conn",
            ForwardStatus::Connecting,
            TunnelStats::default(),
        ));

        // Only the connected tunnel is sampled.
        let batch = sample_once(&state);
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].0, "up");

        // Sampling is a pure read — repeated passes never change any status
        // (the sampler has NO teardown authority, F1/F21).
        for _ in 0..5 {
            let _ = sample_once(&state);
        }
        assert_eq!(
            state.registry.current_status("up"),
            Some(ForwardStatus::Connected)
        );
        assert_eq!(
            state.registry.current_status("err"),
            Some(ForwardStatus::Error)
        );
        assert_eq!(
            state.registry.current_status("conn"),
            Some(ForwardStatus::Connecting)
        );
    }

    #[tokio::test]
    async fn ensure_sampler_is_idempotent_and_auto_stops() {
        let state = Arc::new(AppState::new_headless());
        state.registry.insert(seed_handle(
            "a",
            ForwardStatus::Connected,
            stats_with_latency(7),
        ));

        // First call starts it; a second call while running is a no-op.
        ensure_sampler(&state);
        assert!(state.registry.is_sampler_running());
        ensure_sampler(&state); // idempotent — must not spawn a second sampler

        // Tunnel goes away → within a couple of ticks the sampler auto-stops and
        // releases the slot.
        state.registry.remove("a");
        let deadline = std::time::Instant::now() + Duration::from_secs(12);
        while state.registry.is_sampler_running() {
            assert!(
                std::time::Instant::now() < deadline,
                "sampler should auto-stop once no tunnels are connected"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}
