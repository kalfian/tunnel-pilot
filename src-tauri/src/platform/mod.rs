//! Platform integration: dock/taskbar visibility, autostart sync, and
//! notifications (spec 03 §§12,13,15).
//!
//! All of it is desktop-only. The headless CLI build keeps just a no-op
//! [`notify`] facade so the SSH engine calls it unconditionally; the daemon's
//! log stream (stderr → journald/launchd) is the record of connects and errors.

#[cfg(feature = "gui")]
pub mod autostart;
#[cfg(feature = "gui")]
pub mod dock;
#[cfg(all(feature = "gui", target_os = "macos"))]
pub mod macos;
#[cfg(feature = "gui")]
pub mod notify;

/// Headless notifications: there is no desktop to notify. The engine already
/// logs every connect and terminal error through `tracing`.
#[cfg(not(feature = "gui"))]
pub mod notify {
    use crate::state::AppState;

    /// No-op outside the desktop build.
    pub fn notify_connected(_state: &AppState, _name: &str) {}

    /// No-op outside the desktop build.
    pub fn notify_error(_state: &AppState, _name: &str, _error: &str) {}
}
