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
use std::sync::atomic::{AtomicBool, Ordering};
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
            | (Error, Disconnecting) // command: user disconnect while parked in error
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
/// never held across an `.await` doing network I/O). A second small `Mutex`
/// tracks ids whose supervisor is mid-launch ("starting") so that
/// [`TunnelRegistry::try_begin_start`] can reserve an id atomically and prevent
/// two concurrent `connect_forward`s from spawning duplicate supervisors (F33).
#[derive(Default)]
pub struct TunnelRegistry {
    inner: Mutex<HashMap<TunnelId, TunnelHandle>>,
    /// Ids reserved by an in-flight `connect_forward` before its handle lands in
    /// `inner`. Ordering rule (no deadlock): `try_begin_start` is the ONLY method
    /// that holds both locks, and always takes `inner` first, then `starting`.
    starting: Mutex<std::collections::HashSet<TunnelId>>,
    /// Whether the single shared stats sampler task (`ssh/health.rs`, M2) is
    /// live. Guards auto-start (exactly one sampler regardless of tunnel count,
    /// spec 03 §2) — NOT a lock, just a cheap presence flag.
    sampler_running: AtomicBool,
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

    fn lock_starting(&self) -> MutexGuard<'_, std::collections::HashSet<TunnelId>> {
        self.starting.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Atomically reserve `id` for a starting supervisor (F33). Returns `true`
    /// iff the id is neither already live (in `inner`) nor already reserved by a
    /// concurrent `connect_forward`; on `true` the id is marked "starting" and
    /// the caller MUST eventually [`insert`](Self::insert) (which clears the
    /// reservation) or call [`finish_start`](Self::finish_start) on failure.
    /// This closes the check-then-insert race that would otherwise let a second
    /// concurrent connect orphan the first supervisor (leaked task + bound port).
    pub fn try_begin_start(&self, id: &str) -> bool {
        let live = self.lock();
        let mut starting = self.lock_starting();
        if live.contains_key(id) || starting.contains(id) {
            return false;
        }
        starting.insert(id.to_string());
        true
    }

    /// Clear a "starting" reservation without inserting a handle (start aborted).
    pub fn finish_start(&self, id: &str) {
        self.lock_starting().remove(id);
    }

    /// Insert the live handle and clear any "starting" reservation for its id
    /// (the reservation and the handle never coexist as a gap, F33).
    pub fn insert(&self, handle: TunnelHandle) {
        let id = handle.id.clone();
        self.lock().insert(id.clone(), handle);
        self.lock_starting().remove(&id);
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
        // Cancel the outgoing attempt token so any lingering children of the
        // previous attempt tear down explicitly/token-driven (F34).
        h.attempt_cancel.cancel();
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
        // Cancel the OUTGOING token first (F34): on a reconnect transition this
        // explicitly reaps the previous attempt's forward children via their
        // `attempt_cancel.cancelled()` arm, rather than relying on the session
        // disconnect erroring their channels — so `active_connections` isn't
        // left briefly inflated and teardown is token-driven.
        h.attempt_cancel.cancel();
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
        // send_replace updates the value regardless of whether any receiver is
        // alive (plain `send` would fail-and-not-update with zero receivers).
        h.status.send_replace(new);
        StatusOutcome { applied: true, new }
    }

    /// Enter terminal `error` AND check-and-clear the retry flag in the SAME
    /// critical section (F29, spec 03 §1). If `retry_already_requested`, the
    /// supervisor must loop into a new attempt instead of parking.
    pub fn begin_terminal_error(
        &self,
        id: &str,
        last_error: Option<String>,
    ) -> TerminalErrorOutcome {
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
            h.status.send_replace(ForwardStatus::Error);
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

    /// `(id, runtime)` for every live tunnel — the runtime half of the
    /// `app_hydrate` snapshot (spec 04 §8). Reads only.
    pub fn all_runtimes(&self) -> Vec<(TunnelId, ForwardRuntime)> {
        self.lock()
            .iter()
            .map(|(id, h)| (id.clone(), h.runtime()))
            .collect()
    }

    /// Claim the shared-sampler slot (M2, spec 03 §2). Returns `true` iff the
    /// caller transitioned it from stopped→running and MUST therefore spawn the
    /// sampler; `false` means one is already live (idempotent auto-start).
    pub fn try_start_sampler(&self) -> bool {
        !self.sampler_running.swap(true, Ordering::SeqCst)
    }

    /// Release the shared-sampler slot (the sampler calls this before it exits
    /// on seeing no connected tunnels, then re-checks to close the auto-restart
    /// race — see `ssh/health.rs`).
    pub fn stop_sampler(&self) {
        self.sampler_running.store(false, Ordering::SeqCst);
    }

    /// Whether a shared sampler is currently claimed (tests / diagnostics).
    pub fn is_sampler_running(&self) -> bool {
        self.sampler_running.load(Ordering::SeqCst)
    }
}

/// Build a minimal live `TunnelHandle` for a given status — test-only, used by
/// command-layer tests (e.g. `update_forward` force-disconnect) that need a
/// tunnel to appear "live" in the registry without a real SSH session. The
/// supervisor `JoinHandle` is an already-complete no-op task so a disconnect's
/// `join.await` returns immediately.
#[cfg(test)]
pub(crate) fn fake_handle(id: &str, status: ForwardStatus) -> TunnelHandle {
    let parent = CancellationToken::new();
    let attempt = parent.child_token();
    let (status_tx, _rx) = watch::channel(status);
    let (_stats_tx, stats_rx) = watch::channel(TunnelStats::default());
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

    // ---- handle-based state-machine tests (need a tokio runtime for the
    // dummy JoinHandle + watch/Notify) ----

    fn make_handle(id: &str, initial: ForwardStatus) -> TunnelHandle {
        super::fake_handle(id, initial)
    }

    #[tokio::test]
    async fn lifecycle_happy_path() {
        let reg = TunnelRegistry::new();
        reg.insert(make_handle("a", Disconnected));
        // connect -> connecting -> connected -> disconnecting -> disconnected.
        assert!(reg.set_status("a", Connecting, None).applied);
        assert!(reg.set_status("a", Connected, None).applied);
        assert!(reg.set_status("a", Disconnecting, None).applied);
        assert!(reg.set_status("a", Disconnected, None).applied);
        assert_eq!(reg.current_status("a"), Some(Disconnected));
    }

    #[tokio::test]
    async fn clicks_during_disconnecting_are_no_ops() {
        let reg = TunnelRegistry::new();
        reg.insert(make_handle("a", Disconnecting));
        // F23: connect/error/connected while disconnecting are all dropped.
        assert!(!reg.set_status("a", Connecting, None).applied);
        assert!(!reg.set_status("a", Error, Some("x".into())).applied);
        assert!(!reg.set_status("a", Connected, None).applied);
        assert_eq!(reg.current_status("a"), Some(Disconnecting));
        // Only disconnecting -> disconnected advances it.
        assert!(reg.set_status("a", Disconnected, None).applied);
    }

    #[tokio::test]
    async fn session_drop_racing_user_disconnect_never_flashes_error() {
        // F28: user disconnect moved connected -> disconnecting; a session drop
        // arriving after must NOT produce disconnecting -> error.
        let reg = TunnelRegistry::new();
        reg.insert(make_handle("a", Connected));
        assert!(reg.set_status("a", Disconnecting, None).applied);
        assert!(!reg.set_status("a", Error, Some("drop".into())).applied);
        assert_eq!(reg.current_status("a"), Some(Disconnecting));
    }

    #[tokio::test]
    async fn disconnect_while_connecting_is_allowed_f31() {
        // F31: a user disconnecting a still-connecting tunnel is not stranded.
        let reg = TunnelRegistry::new();
        reg.insert(make_handle("a", Connecting));
        assert!(reg.set_status("a", Disconnecting, None).applied);
        assert!(reg.set_status("a", Disconnected, None).applied);
    }

    #[tokio::test]
    async fn two_level_token_semantics_f6() {
        let reg = TunnelRegistry::new();
        reg.insert(make_handle("a", Connected));
        let parent = reg.parent_token("a").unwrap();

        // A reconnect swaps only the child token; the parent stays alive.
        let t1 = reg.mint_fresh_attempt("a").unwrap();
        assert!(!parent.is_cancelled() && !t1.is_cancelled());
        t1.cancel(); // attempt reset
        assert!(t1.is_cancelled());
        assert!(
            !parent.is_cancelled(),
            "attempt cancel must NOT touch parent"
        );

        let t2 = reg.mint_fresh_attempt("a").unwrap();
        assert!(!t2.is_cancelled(), "fresh child of a live parent");

        // Cancelling the parent kills the current child too.
        reg.cancel_parent("a");
        assert!(parent.is_cancelled() && t2.is_cancelled());
    }

    #[tokio::test]
    async fn retry_only_acts_when_parked_in_error_f27c() {
        let reg = TunnelRegistry::new();
        reg.insert(make_handle("a", Connecting));
        // Not in error → request_retry is a no-op (no flag, no wakeup).
        assert!(reg.request_retry("a").is_none());
        assert!(!reg.take_retry_requested("a"));

        // Move to error → retry is accepted and the flag becomes the truth.
        reg.insert(make_handle("b", Error));
        assert!(reg.request_retry("b").is_some());
        assert!(reg.take_retry_requested("b"));
        // Consumed once.
        assert!(!reg.take_retry_requested("b"));
    }

    #[tokio::test]
    async fn retry_racing_final_failure_is_honored_f29() {
        // Enter terminal error (defensive in-section check clears nothing yet),
        // then a retry arrives; the load-bearing take_retry_requested sees it.
        let reg = TunnelRegistry::new();
        reg.insert(make_handle("a", Connected));
        let term = reg.begin_terminal_error("a", Some("final".into()));
        assert!(term.applied);
        assert!(!term.retry_already_requested);
        // retry the instant after error was set:
        assert!(reg.request_retry("a").is_some());
        // supervisor's on-wake re-check honors it — never lost:
        assert!(reg.take_retry_requested("a"));
    }
}
