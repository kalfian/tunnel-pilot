//! The connecting animation for the menu-bar icon (spec 03 §10, connecting
//! indicator): a monochrome "loading dots under the logo" ticker. While any
//! tunnel is in a transitional state (connecting/disconnecting) the tray icon
//! cycles the app glyph with a progressing row of dots (`.` → `..` → `...` → …)
//! so the user reads "something is working"; when nothing is transitional
//! anymore the ticker stops and [`super::menu::rebuild_now`] settles the static
//! count/idle icon.
//!
//! Frames advance forward at a constant interval, giving a discrete, rhythmic
//! "tick-tick-tick" cadence (not a fade). A single guarded tokio task drives it:
//! [`ConnectingAnimator::start`] uses an atomic compare-exchange so the task can
//! never double-run; `stop` flips the flag so the task's next loop turn exits —
//! no leak, no busy-spin when idle. Every frame is painted on the AppKit main
//! thread via `run_on_main_thread`, and the queued closure re-checks the running
//! flag so a frame that lands after a stop can never overwrite the settled icon.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;

/// Interval per tick — a steady ~450 ms so the dots advance in a regular,
/// mechanical tick-tick-tick cadence.
const FRAME_INTERVAL: Duration = Duration::from_millis(450);

/// Controls the single connecting-ticker task. Cheap to clone (shared flag).
#[derive(Clone, Default)]
pub struct ConnectingAnimator {
    running: Arc<AtomicBool>,
}

impl ConnectingAnimator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the ticker task is currently running (used by the icon-paint guard).
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Start the ticker if not already running (idempotent). The spawned task
    /// advances the dot frame every [`FRAME_INTERVAL`] until [`stop`] is
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

    /// Stop the ticker (idempotent). The task exits on its next loop turn; the
    /// caller ([`super::menu::rebuild_now`]) then paints the settled static icon.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }
}

/// Dispatch one dot frame to the main thread. The queued closure re-checks
/// `running` so a frame that lands after a `stop` (and after the static icon was
/// re-applied) is dropped instead of freezing the icon on a stale frame.
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
