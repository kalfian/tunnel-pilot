//! Per-tunnel stats accounting (spec 03 §6).
//!
//! Ownership split (F21): DURABLE byte/conn counters live in `StatsInner`
//! (atomics), written by the copy/accept tasks. Latency is written by the
//! SUPERVISOR's channel-open RTT probe. The supervisor rolls a snapshot into a
//! `watch::Sender<TunnelStats>` (in `TunnelHandle`) that the shared sampler
//! reads (M2). The dead-channel teardown counter is NOT here — it is a
//! per-ATTEMPT `Arc<AtomicUsize>` minted with each attempt (F30, spec 03 §1).

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::state::models::TunnelStats;

/// Durable per-tunnel runtime counters (spec 03 §6). Lives behind an `Arc` so
/// the supervisor, copy tasks, and the accept loop can all update it.
#[derive(Debug)]
pub struct StatsInner {
    /// Live local sockets currently forwarded (inc on open, dec on close).
    pub active_connections: AtomicUsize,
    /// Cumulative bytes local→remote.
    pub bytes_up: AtomicU64,
    /// Cumulative bytes remote→local.
    pub bytes_down: AtomicU64,
    /// Last measured RTT in ms; `0` = none. Written ONLY by the supervisor's
    /// channel-open probe (F1/F21).
    pub last_latency_ms: AtomicU64,
    /// Set on connect, cleared on disconnect. Monotonic `Instant` for uptime
    /// deltas (spec 03 Conventions — display uses wall clock elsewhere).
    pub connected_since: Mutex<Option<Instant>>,
    /// Wall-clock RFC3339 string captured on connect, for the `connectedSince`
    /// wire field (display only; None while disconnected).
    pub connected_since_wall: Mutex<Option<String>>,
    // NOTE (F30): the dead-channel teardown counter is per-attempt (see engine.rs).
    // NOTE (F1): no `ping_failures` field — liveness is owned by russh keepalive (§2).
}

impl Default for StatsInner {
    fn default() -> Self {
        Self {
            active_connections: AtomicUsize::new(0),
            bytes_up: AtomicU64::new(0),
            bytes_down: AtomicU64::new(0),
            last_latency_ms: AtomicU64::new(0),
            connected_since: Mutex::new(None),
            connected_since_wall: Mutex::new(None),
        }
    }
}

impl StatsInner {
    /// Mark the tunnel connected: record the monotonic + wall-clock start.
    pub fn mark_connected(&self) {
        if let Ok(mut g) = self.connected_since.lock() {
            *g = Some(Instant::now());
        }
        if let Ok(mut g) = self.connected_since_wall.lock() {
            *g = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    /// Clear connect markers on disconnect/teardown.
    pub fn mark_disconnected(&self) {
        if let Ok(mut g) = self.connected_since.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.connected_since_wall.lock() {
            *g = None;
        }
    }

    /// Record a fresh latency sample (supervisor RTT probe).
    pub fn set_latency(&self, latency: std::time::Duration) {
        self.last_latency_ms
            .store(latency.as_millis() as u64, Ordering::Relaxed);
    }

    /// Build the wire snapshot the sampler emits (spec 04 §5).
    pub fn snapshot(&self) -> TunnelStats {
        let latency = self.last_latency_ms.load(Ordering::Relaxed);
        let connected_since = self
            .connected_since_wall
            .lock()
            .ok()
            .and_then(|g| g.clone());
        TunnelStats {
            active_connections: self.active_connections.load(Ordering::Relaxed),
            total_bytes_up: self.bytes_up.load(Ordering::Relaxed),
            total_bytes_down: self.bytes_down.load(Ordering::Relaxed),
            last_ping_latency_ms: if latency == 0 { None } else { Some(latency) },
            connected_since,
        }
    }
}
