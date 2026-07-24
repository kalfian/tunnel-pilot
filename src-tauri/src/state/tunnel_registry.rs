//! Per-tunnel runtime registry: `map<TunnelId, TunnelHandle>` (spec 03 §§1,5).
//!
//! Holds the two-level cancellation hierarchy (durable `parent_cancel` +
//! per-attempt `attempt_cancel = parent.child_token()`, F6), the STABLE
//! `JoinHandle` (F21), the lock-guarded `retry_requested` flag (F29 — the
//! source of truth, NOT a Notify permit), the wakeup-only `retry_notify`, the
//! status `watch` sender, the stats cell, and the durable `StatsInner`.
//!
//! **Status is written ONLY via the guarded [`TunnelRegistry::set_status`]**
//! under the registry lock, which enforces the transition table and no-ops any
//! illegal `(current, new)` pair (F23/F28/F31).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::ssh::stats::StatsInner;
use crate::state::models::{ForwardRuntime, ForwardStatus, TunnelId, TunnelStats};

/// Whether `(from → to)` is an allowed status transition (spec 03 §1 table).
///
/// Pure and total so it can be unit-tested over ALL `(current, new)` pairs
/// (F28). Any pair not listed here is illegal → `set_status` no-ops it.
/// Deliberate absences: `disconnecting → error/connected/connecting` are NOT
/// allowed (a session drop that races a user disconnect must not flash
/// `disconnecting → error`). `connecting → disconnecting` IS allowed (F31 — a
/// user disconnecting a still-connecting tunnel must not be stranded).
pub fn transition_allowed(from: ForwardStatus, to: ForwardStatus) -> bool {
    use ForwardStatus::*;
    matches!(
        (from, to),
        (Disconnected, Connecting)      // supervisor: connect starts
            | (Connecting, Connected)   // supervisor: bind+connect+auth+accept up
            | (Connecting, Error)       // supervisor: connect/auth failure
            | (Connecting, Disconnecting) // command: user disconnect while connecting (F31)
            | (Connected, Error)        // supervisor: session drop / 3 forward failures (F26)
            | (Connected, Disconnecting) // command: user disconnect
            | (Disconnecting, Disconnected) // command: cleanup done
            | (Error, Connecting)       // supervisor: retry / auto-reconnect next attempt
            | (Error, Disconnecting)    // command: user disconnect while parked in error
    )
}

/// Live per-tunnel runtime handle (spec 03 §1 "Rust approach"). One per active
/// tunnel; removed only on `disconnect`/`delete` after the `JoinHandle` is
/// awaited (so the local port is released before the entry disappears, F21).
pub struct TunnelHandle {
    pub id: TunnelId,
    /// Durable, one per tunnel; cancelled ONLY by disconnect/delete/quit (F6).
    pub parent_cancel: CancellationToken,
    /// Child of `parent_cancel`; replaced each (re)connect attempt (F6).
    pub attempt_cancel: CancellationToken,
    /// Supervisor task — STABLE for the tunnel's whole life (F21/F23).
    pub join: JoinHandle<()>,
    /// Live status. Written ONLY via [`TunnelRegistry::set_status`] (F23/F28).
    pub status: watch::Sender<ForwardStatus>,
    /// Last error message surfaced (for `ForwardRuntime.lastError`).
    pub last_error: Option<String>,
    /// Pending-retry SOURCE OF TRUTH — guarded by the registry lock (F29).
    pub retry_requested: bool,
    /// WAKEUP ONLY (never the truth): `retry_forward` pokes it after setting the
    /// flag; the parked supervisor re-checks the flag under the lock (F29).
    pub retry_notify: Arc<Notify>,
    /// Sleep/resume nudge (NIT-1): `request_wake_probe` pokes it so the
    /// supervisor runs an immediate RTT probe and reconnects if dead (§4). Wired
    /// to the wake watchdog in M2; the supervisor's `select!` arm exists in M1.
    pub wake_notify: Arc<Notify>,
    /// Sampler reads this (M2); the supervisor owns the matching `Sender` and
    /// publishes latency + derived stats into it (§6).
    pub stats_cell: watch::Receiver<TunnelStats>,
    /// Durable atomics for byte/conn counters (updated by copy/accept tasks).
    pub stats: Arc<StatsInner>,
}

impl TunnelHandle {
    /// Build the combined runtime view for `get_forward_runtime` (spec 04 §5).
    pub fn runtime(&self) -> ForwardRuntime {
        ForwardRuntime {
            status: *self.status.borrow(),
            stats: self.stats.snapshot(),
            last_error: self.last_error.clone(),
        }
    }
}

/// Outcome of a guarded status write — the caller emits `tunnel://status`
/// outside the lock iff `applied`.
#[derive(Debug, Clone, Copy)]
pub struct StatusOutcome {
    pub applied: bool,
    pub new: ForwardStatus,
}

/// Outcome of entering terminal `error` while also check-and-clearing the retry
/// flag in the SAME critical section (F29). If `retry_already_requested`, the
/// supervisor must NOT park — a retry already arrived; loop to a new attempt.
#[derive(Debug, Clone, Copy)]
pub struct TerminalErrorOutcome {
    pub applied: bool,
    pub retry_already_requested: bool,
}

/// The registry: a single briefly-held `Mutex` guards the map (spec 03 §5 —
/// never held across an `.await` doing network I/O).
#[derive(Default)]
pub struct TunnelRegistry {
    inner: Mutex<HashMap<TunnelId, TunnelHandle>>,
}

impl TunnelRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lock the map, recovering from poisoning rather than panicking (a poisoned
    /// registry only means a prior thread panicked mid-mutation; the map itself
    /// stays consistent for our all-or-nothing ops). Held only for map work.
    fn lock(&self) -> MutexGuard<'_, HashMap<TunnelId, TunnelHandle>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert(&self, handle: TunnelHandle) {
        self.lock().insert(handle.id.clone(), handle);
    }

    /// Remove and return a handle (caller awaits its `JoinHandle` first, F21).
    pub fn remove(&self, id: &str) -> Option<TunnelHandle> {
        self.lock().remove(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.lock().contains_key(id)
    }

    pub fn current_status(&self, id: &str) -> Option<ForwardStatus> {
        self.lock().get(id).map(|h| *h.status.borrow())
    }

    pub fn runtime(&self, id: &str) -> Option<ForwardRuntime> {
        self.lock().get(id).map(|h| h.runtime())
    }

    pub fn parent_token(&self, id: &str) -> Option<CancellationToken> {
        self.lock().get(id).map(|h| h.parent_cancel.clone())
    }

    pub fn attempt_token(&self, id: &str) -> Option<CancellationToken> {
        self.lock().get(id).map(|h| h.attempt_cancel.clone())
    }

    pub fn retry_notify(&self, id: &str) -> Option<Arc<Notify>> {
        self.lock().get(id).map(|h| h.retry_notify.clone())
    }

    pub fn wake_notify(&self, id: &str) -> Option<Arc<Notify>> {
        self.lock().get(id).map(|h| h.wake_notify.clone())
    }

    /// Set the retry flag AND mint a fresh attempt token in ONE locked section,
    /// but ONLY when the tunnel is parked in `error` (F27c). Returns the
    /// wakeup `Notify` to poke (outside the lock) iff the retry was accepted.
    pub fn request_retry(&self, id: &str) -> Option<Arc<Notify>> {
        let mut g = self.lock();
        let h = g.get_mut(id)?;
        if *h.status.borrow() != ForwardStatus::Error {
            return None; // no-op unless parked in error
        }
        h.retry_requested = true;
        let child = h.parent_cancel.child_token();
        h.attempt_cancel = child;
        Some(h.retry_notify.clone())
    }

    pub fn stats(&self, id: &str) -> Option<Arc<StatsInner>> {
        self.lock().get(id).map(|h| h.stats.clone())
    }

    /// Cancel the durable parent token → ends the whole supervisor loop (F6).
    pub fn cancel_parent(&self, id: &str) {
        if let Some(h) = self.lock().get(id) {
            h.parent_cancel.cancel();
        }
    }

    /// Mint a FRESH per-attempt child token from the parent, store it in the
    /// handle, and return a clone (F6). Called at the top of every attempt and
    /// by `retry_forward`. Leaves the parent untouched.
    pub fn mint_fresh_attempt(&self, id: &str) -> Option<CancellationToken> {
        let mut g = self.lock();
        let h = g.get_mut(id)?;
        let child = h.parent_cancel.child_token();
        h.attempt_cancel = child.clone();
        Some(child)
    }

    /// Set the flag (guarded). Only meaningful together with a `status==error`
    /// check by the caller (F27c).
    pub fn set_retry_requested(&self, id: &str, val: bool) {
        if let Some(h) = self.lock().get_mut(id) {
            h.retry_requested = val;
        }
    }

    /// Check-and-CLEAR the retry flag in one locked step (F29).
    pub fn take_retry_requested(&self, id: &str) -> bool {
        let mut g = self.lock();
        match g.get_mut(id) {
            Some(h) => std::mem::replace(&mut h.retry_requested, false),
            None => false,
        }
    }

    /// The single GUARDED status writer (F23/F28). Applies ONLY transitions in
    /// [`transition_allowed`]; every other `(current, new)` pair is a silent
    /// no-op. Updates `last_error` (set on `Error`, cleared on `Connected`).
    /// Returns whether it applied so the caller can emit `tunnel://status`
    /// exactly once, outside the lock.
    pub fn set_status(
        &self,
        id: &str,
        new: ForwardStatus,
        last_error: Option<String>,
    ) -> StatusOutcome {
        let mut g = self.lock();
        let Some(h) = g.get_mut(id) else {
            return StatusOutcome {
                applied: false,
                new,
            };
        };
        let current = *h.status.borrow();
        if !transition_allowed(current, new) {
            return StatusOutcome {
                applied: false,
                new: current,
            };
        }
        match new {
            ForwardStatus::Error => h.last_error = last_error,
            ForwardStatus::Connected => h.last_error = None,
            _ => {}
        }
        // watch::send only errors if there are no receivers; the status is still
        // the source of truth in the sender, so a send failure is non-fatal.
        let _ = h.status.send(new);
        StatusOutcome { applied: true, new }
    }

    /// Enter terminal `error` AND check-and-clear the retry flag in the SAME
    /// critical section (F29, spec 03 §1). If `retry_already_requested`, the
    /// supervisor must loop into a new attempt instead of parking.
    pub fn begin_terminal_error(&self, id: &str, last_error: Option<String>) -> TerminalErrorOutcome {
        let mut g = self.lock();
        let Some(h) = g.get_mut(id) else {
            return TerminalErrorOutcome {
                applied: false,
                retry_already_requested: false,
            };
        };
        let current = *h.status.borrow();
        let applied = transition_allowed(current, ForwardStatus::Error);
        if applied {
            h.last_error = last_error;
            let _ = h.status.send(ForwardStatus::Error);
        }
        // Defensive in-section check-and-clear (F29 NIT-2): effectively always
        // false because a retry cannot set the flag before status==error (set on
        // the line above under this same lock). The load-bearing check is the
        // on-wake re-check in the supervisor.
        let retry_already_requested = std::mem::replace(&mut h.retry_requested, false);
        TerminalErrorOutcome {
            applied,
            retry_already_requested,
        }
    }

    /// Snapshot of currently-connected tunnels for the shared stats sampler
    /// (M2): `(id, stats_receiver)`. Reads only; no session access (F21).
    pub fn connected_snapshot(&self) -> Vec<(TunnelId, watch::Receiver<TunnelStats>)> {
        self.lock()
            .values()
            .filter(|h| *h.status.borrow() == ForwardStatus::Connected)
            .map(|h| (h.id.clone(), h.stats_cell.clone()))
            .collect()
    }

    /// All live tunnel ids (for `stop_all` / shutdown).
    pub fn all_ids(&self) -> Vec<TunnelId> {
        self.lock().keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ForwardStatus::*;

    const ALL: [ForwardStatus; 5] = [Disconnected, Connecting, Connected, Disconnecting, Error];

    #[test]
    fn transition_table_exhaustive_over_all_pairs() {
        // The complete allow-list (spec 03 §1 table). Everything else is a no-op.
        let allowed = [
            (Disconnected, Connecting),
            (Connecting, Connected),
            (Connecting, Error),
            (Connecting, Disconnecting),
            (Connected, Error),
            (Connected, Disconnecting),
            (Disconnecting, Disconnected),
            (Error, Connecting),
            (Error, Disconnecting),
        ];
        for &from in &ALL {
            for &to in &ALL {
                let expected = allowed.contains(&(from, to));
                assert_eq!(
                    transition_allowed(from, to),
                    expected,
                    "({from:?} -> {to:?}) allow mismatch"
                );
            }
        }
    }

    #[test]
    fn disconnecting_to_error_is_dropped() {
        // F28: a session drop coincident with a user disconnect must not flash
        // disconnecting -> error.
        assert!(!transition_allowed(Disconnecting, Error));
        assert!(!transition_allowed(Disconnecting, Connected));
        assert!(!transition_allowed(Disconnecting, Connecting));
    }

    #[test]
    fn connecting_to_disconnecting_is_allowed() {
        // F31: user disconnecting a still-connecting tunnel is never stranded.
        assert!(transition_allowed(Connecting, Disconnecting));
    }

    #[test]
    fn same_state_is_never_allowed() {
        for &s in &ALL {
            assert!(!transition_allowed(s, s), "{s:?} -> {s:?} must be a no-op");
        }
    }
}
