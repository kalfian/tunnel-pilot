//! Sleep/wake watchdog: monotonic-gap detection (>30s ⇒ sweep + immediate
//! reconnect). Best-effort — the session-future signal is the backstop (F15).
//!
//! A single app-lifetime task ticks every 5s and measures the elapsed gap
//! between ticks. If the gap exceeds 30s the machine likely slept, so it sweeps
//! only the `connected` tunnels and pokes each supervisor's existing
//! `request_wake_probe` (`wake_notify`) — the supervisor runs an immediate RTT
//! probe on its OWN session and, on failure, reconnects at once (bypassing
//! backoff). The watchdog holds NO session (F21) and does not decide teardown.
//!
//! Per F15 this heuristic is NOT assumed reliable: whether monotonic clocks and
//! tokio timers advance across OS sleep is platform-dependent (App Nap, timer
//! coalescing, suspended runtimes). Recovery ultimately leans on the russh
//! session-future signal (F7) as the real backstop even if this never fires.
//! Real-OS-sleep verification is deferred to M6 (F15, spec 07 M2/M6).

use std::sync::Arc;
use std::time::Duration;

use tokio::time::{interval, Instant, MissedTickBehavior};

use crate::ssh::engine;
use crate::state::AppState;

/// Watchdog tick cadence.
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(5);
/// Inter-tick gap above which we infer a sleep/resume (spec 03 §4, v1 parity).
const WAKE_GAP_THRESHOLD: Duration = Duration::from_secs(30);

/// Whether an observed inter-tick `gap` implies the machine likely slept (§4).
/// Pure so the threshold is unit-testable independent of the timer.
fn slept(gap: Duration) -> bool {
    gap > WAKE_GAP_THRESHOLD
}

/// Poke every CONNECTED tunnel's supervisor to probe its session NOW (§4). Reads
/// only connected tunnels — disconnected/errored/connecting ones are untouched.
/// Holds no session; each supervisor probes its own and reconnects if dead.
fn sweep_connected(state: &Arc<AppState>) {
    for (id, _cell) in state.registry.connected_snapshot() {
        engine::request_wake_probe(state, &id);
    }
}

/// Spawn the app-lifetime wake watchdog on the Tauri async runtime (called from
/// `lib.rs` setup, which is not itself inside an async context).
pub fn spawn_wake_watchdog(state: Arc<AppState>) {
    tauri::async_runtime::spawn(watchdog_loop(state));
}

/// The monotonic-gap loop. Uses `tokio::time::Instant` so the gap reflects the
/// runtime's own clock (and is controllable under `tokio::time::pause` in tests).
async fn watchdog_loop(state: Arc<AppState>) {
    let mut tick = interval(WATCHDOG_INTERVAL);
    // After a long suspend the first tick fires promptly; Delay then spaces the
    // next tick a full interval out so we sweep once per resume, not in a burst.
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut last = Instant::now();
    tracing::debug!("wake watchdog started");

    loop {
        tick.tick().await;
        let now = Instant::now();
        let gap = now.duration_since(last);
        last = now;
        if slept(gap) {
            tracing::info!(
                gap_secs = gap.as_secs(),
                "wake watchdog: >30s gap -> probing connected tunnels"
            );
            sweep_connected(&state);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_threshold_is_exactly_30s() {
        assert!(!slept(Duration::from_secs(0)));
        assert!(!slept(Duration::from_secs(5)));
        assert!(!slept(Duration::from_secs(30)), "30s exactly is not a wake");
        assert!(slept(Duration::from_secs(31)), "just over 30s is a wake");
        assert!(slept(Duration::from_secs(600)));
    }

    #[tokio::test]
    async fn sweep_touches_no_tunnels_when_none_connected() {
        // With an empty registry the sweep is a no-op and must not panic.
        let state = Arc::new(AppState::new_headless());
        sweep_connected(&state);
    }
}
