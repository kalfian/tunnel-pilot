//! The connecting animation for the menu-bar icon (spec 03 §10, connecting
//! indicator): big bright (non-template) ticking dots in the bottom-right corner
//! of the logo glyph — `●` → `●●` → `●●●` → `●` … growing left→right. While any
//! tunnel is in a transitional state (connecting/disconnecting) the corner dots
//! tick continuously; when nothing is transitional anymore the ticker goes idle
//! and [`super::menu::rebuild_now`] settles the static (template) count/idle icon.
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
//!
//! ## App Nap (macOS) — why the tick used to lag
//! Tunnel Pilot runs as a background menu-bar (Accessory) agent. macOS App Nap
//! coalesces such an app's timers and throttles its main run loop, so the tokio
//! tick fired late and the `run_on_main_thread` icon paints landed slower than
//! [`FRAME_INTERVAL`] — the animation looked far slower than its nominal rate.
//! While the ticker is active it holds an `NSProcessInfo` activity assertion
//! ([`app_nap`]) that disables App Nap (without keeping the system awake), so the
//! tick stays on time; the assertion is released the moment it goes idle.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tauri::AppHandle;
use tokio::sync::Notify;

/// Interval per tick — a snappy ~220 ms so the corner dots advance in a brisk
/// but still-discrete tick-tick-tick cadence for the full connecting duration.
const FRAME_INTERVAL: Duration = Duration::from_millis(220);

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
            // Held (macOS) only while ticking so App Nap can't coalesce the timer
            // / throttle the paints; dropped on idle to release the assertion.
            #[cfg(target_os = "macos")]
            let mut nap: Option<app_nap::AppNapGuard> = None;
            #[cfg(target_os = "macos")]
            let mut nap_tried = false;
            loop {
                if !active.load(Ordering::SeqCst) {
                    // Idle: reset so the next connect starts at one dot, then
                    // block until woken (no spin, no wasted wakeups).
                    #[cfg(target_os = "macos")]
                    {
                        nap = None;
                        nap_tried = false;
                    }
                    frame = 0;
                    wake.notified().await;
                    continue;
                }
                // Disable App Nap for the duration of this connect (attempt once).
                #[cfg(target_os = "macos")]
                if nap.is_none() && !nap_tried {
                    nap = app_nap::AppNapGuard::begin("Tunnel Pilot tray connecting animation");
                    nap_tried = true;
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

/// macOS App Nap suppression via an `NSProcessInfo` activity assertion. Keeping
/// this alive stops App Nap from coalescing the tick timer / throttling the tray
/// paints while a tunnel is connecting. A minimal `objc2` `msg_send!` shim
/// (objc2 is already in the tree via Tauri); the activity API is documented as
/// thread-safe, so we can begin/end it from the ticker task's thread.
#[cfg(target_os = "macos")]
mod app_nap {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};

    /// `NSActivityUserInitiated & ~NSActivityIdleSystemSleepDisabled` — prevents
    /// App Nap (so periodic tray updates stay on time) WITHOUT keeping the system
    /// awake / display on.
    const NS_ACTIVITY_USER_INITIATED_ALLOWING_IDLE_SLEEP: u64 = 0x00FF_FFFF;

    /// Holds one `NSProcessInfo` activity assertion; releases it on drop.
    pub struct AppNapGuard {
        /// Retained activity token returned by `beginActivityWithOptions:reason:`.
        token: *mut AnyObject,
    }

    // The token is an ObjC object we own (retained). NSProcessInfo activity
    // objects are thread-safe and we only retain/endActivity/release it, so
    // moving the guard onto the async task's thread is sound.
    unsafe impl Send for AppNapGuard {}

    impl AppNapGuard {
        /// Begin an activity assertion; `None` if the ObjC calls yield null.
        pub fn begin(reason: &str) -> Option<Self> {
            let c_reason = std::ffi::CString::new(reason).ok()?;
            // Scope the autorelease pool so the autoreleased NSString + activity
            // token don't leak on this (non-main) thread; we `retain` the token
            // inside the pool so it survives the drain.
            objc2::rc::autoreleasepool(|_| unsafe {
                let pi: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
                if pi.is_null() {
                    return None;
                }
                let reason_ns: *mut AnyObject =
                    msg_send![class!(NSString), stringWithUTF8String: c_reason.as_ptr()];
                if reason_ns.is_null() {
                    return None;
                }
                let token: *mut AnyObject = msg_send![
                    pi,
                    beginActivityWithOptions: NS_ACTIVITY_USER_INITIATED_ALLOWING_IDLE_SLEEP,
                    reason: reason_ns,
                ];
                if token.is_null() {
                    return None;
                }
                let token: *mut AnyObject = msg_send![token, retain];
                Some(Self { token })
            })
        }
    }

    impl Drop for AppNapGuard {
        fn drop(&mut self) {
            objc2::rc::autoreleasepool(|_| unsafe {
                let pi: *mut AnyObject = msg_send![class!(NSProcessInfo), processInfo];
                if !pi.is_null() {
                    let _: () = msg_send![pi, endActivity: self.token];
                }
                let _: () = msg_send![self.token, release];
            });
        }
    }
}
