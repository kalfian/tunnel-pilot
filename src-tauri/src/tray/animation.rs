//! The connecting-pulse animation for the menu-bar icon (spec 03 §10, connecting
//! indicator). While any tunnel is in a transitional state
//! (connecting/disconnecting) the tray icon gently pulses in amber so the user
//! reads "something is working"; when nothing is transitional anymore the pulse
//! stops and [`super::menu::rebuild_now`] settles the static count/idle icon.
//!
//! A single guarded tokio task drives the pulse. [`ConnectingAnimator::start`]
//! uses an atomic compare-exchange so the task can never double-run; `stop`
//! flips the flag so the task's next loop turn exits — no leak, no busy-spin
//! when idle. Every frame is painted on the AppKit main thread via
//! `run_on_main_thread`, and the queued closure re-checks the running flag so a
//! frame that lands after a stop can never overwrite the freshly-settled icon.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

/// Frame cadence for the pulse — inside the 400–600 ms "reads as working" band.
const FRAME_INTERVAL: Duration = Duration::from_millis(500);

/// Controls the single connecting-pulse task. Cheap to clone (shared flag).
#[derive(Clone, Default)]
pub struct ConnectingAnimator {
    running: Arc<AtomicBool>,
}

impl ConnectingAnimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the pulse task is currently running (used by the icon-paint guard).
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Start the pulse if not already running (idempotent). The spawned task
    /// advances the amber frame every [`FRAME_INTERVAL`] until [`stop`] is
    /// called, then exits cleanly.
    ///
    /// [`stop`]: Self::stop
    pub fn start(&self, app: AppHandle) {
        // compare_exchange is the double-run guard: only the transition
        // false→true spawns a task; a concurrent/repeat start is a no-op.
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }

        let running = self.running.clone();
        tauri::async_runtime::spawn(async move {
            let mut frame = 0usize;
            while running.load(Ordering::SeqCst) {
                paint_frame(&app, running.clone(), frame);
                frame = (frame + 1) % super::icon::CONNECTING_FRAMES;
                tokio::time::sleep(FRAME_INTERVAL).await;
            }
        });
    }

    /// Stop the pulse (idempotent). The task exits on its next loop turn; the
    /// caller ([`super::menu::rebuild_now`]) then paints the settled static icon.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Dispatch one amber frame to the main thread. The queued closure re-checks
/// `running` so a frame that lands after a `stop` (and after the static icon was
/// re-applied) is dropped instead of freezing the icon on a stale amber frame.
fn paint_frame(app: &AppHandle, running: Arc<AtomicBool>, frame: usize) {
    let app_main = app.clone();
    let dispatch = app.run_on_main_thread(move || {
        if !running.load(Ordering::SeqCst) {
            return;
        }
        super::icon::set_connecting_frame(&app_main, super::TRAY_ID, frame);
    });
    if let Err(e) = dispatch {
        tracing::error!(error = %e, "failed to dispatch connecting frame to main thread");
    }
}
