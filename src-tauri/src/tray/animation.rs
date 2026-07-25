//! The connecting animation for the menu-bar icon (spec 03 §10, connecting
//! indicator): the badge shows ticking dots in the SAME pill/position as the
//! connected count badge — `·` → `··` → `···` → `·` … — instead of a number.
//! While any tunnel is in a transitional state (connecting/disconnecting) the
//! badge dots tick continuously; when nothing is transitional anymore the ticker
//! goes idle and [`super::menu::rebuild_now`] settles the static count/idle icon.
//!
//! ## Continuous-ticking guarantee (the "stuck" fix)
//! A **single** tokio task is spawned once ([`ConnectingAnimator::spawn`]) and
//! lives for the whole app. It is driven purely by an `active` flag (== "any
//! tunnel transitional"), NOT by start/stop that spawn/join tasks — so a status
//! event or `rebuild_now` mid-connect can never kill the timer or double-run it:
//!
//! - While `active`, the task paints the next frame every [`FRAME_INTERVAL`] and
//!   keeps going until `active` clears — it never exits mid-connect.
//! - While idle, the task blocks on a `Notify` (no busy-spin, no wasted wakeups).
//! - [`set_active`] only wakes the task on an actual edge, so repeated
//!   same-state rebuilds during connecting neither accelerate nor stop the tick.
//! - The task is the sole icon writer while `active`; `rebuild_now` skips the
//!   static icon set while transitional, and every frame is painted on the
//!   AppKit main thread with a guard that drops any frame landing after the
//!   ticker went idle — so it can never overwrite the freshly-settled icon.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;
use tokio::sync::Notify;

/// Interval per tick — a steady ~450 ms so the dots advance in a regular,
/// mechanical tick-tick-tick cadence for the full connecting duration.
const FRAME_INTERVAL: Duration = Duration::from_millis(450);

/// Controls the single connecting-ticker task. Cheap to clone (shared state).
#[derive(Clone)]
pub struct ConnectingAnimator {
    /// Whether any tunnel is transitional — the task ticks iff this is set.
    active: Arc<AtomicBool>,
    /// Wakes the task on an `active` edge (idle→tick or tick→idle).
    wake: Arc<Notify>,
    /// Guards `spawn` so the long-lived task is created at most once.
    spawned: Arc<AtomicBool>,
}

impl Default for ConnectingAnimator {
    fn default() -> Self {
        Self::new()
    }
}

impl ConnectingAnimator {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            wake: Arc::new(Notify::new()),
            spawned: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Whether the ticker is currently active (used by the icon-paint guard).
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// Spawn the single long-lived ticker task (idempotent — spawns at most
    /// once, guarded by an atomic swap). Call once from `spawn_tray_sync`.
    pub fn spawn(&self, app: AppHandle) {
        // swap→true: only the first caller (false→true) spawns the task.
        if self.spawned.swap(true, Ordering::SeqCst) {
            return;
        }
        let active = self.active.clone();
        let wake = self.wake.clone();
        tauri::async_runtime::spawn(async move {
            let mut frame = 0usize;
            loop {
                if !active.load(Ordering::SeqCst) {
                    // Idle: reset so the next connect starts at one dot, then
                    // block until woken (no spin, no wasted wakeups).
                    frame = 0;
                    wake.notified().await;
                    continue;
                }
                paint_frame(&app, &active, frame);
                frame = (frame + 1) % super::icon::CONNECTING_FRAMES;
                // Sleep the tick, but wake immediately if `active` flips so a
                // settle is responsive.
                tokio::select! {
                    _ = tokio::time::sleep(FRAME_INTERVAL) => {}
                    _ = wake.notified() => {}
                }
            }
        });
    }

    /// Set whether any tunnel is transitional. Idempotent; only wakes the task
    /// on an actual edge, so repeated same-state rebuilds during a connect
    /// neither accelerate nor stop the tick.
    pub fn set_active(&self, active: bool) {
        if self.active.swap(active, Ordering::SeqCst) != active {
            self.wake.notify_one();
        }
    }
}

/// Dispatch one dot frame to the main thread. The queued closure re-checks
/// `active` so a frame that lands after the ticker went idle (and after the
/// static icon was re-applied) is dropped instead of freezing on a stale frame.
fn paint_frame(app: &AppHandle, active: &Arc<AtomicBool>, frame: usize) {
    let app_main = app.clone();
    let active = active.clone();
    let dispatch = app.run_on_main_thread(move || {
        if !active.load(Ordering::SeqCst) {
            return;
        }
        super::icon::set_connecting_frame(&app_main, super::TRAY_ID, frame);
    });
    if let Err(e) = dispatch {
        tracing::error!(error = %e, "failed to dispatch connecting frame to main thread");
    }
}
