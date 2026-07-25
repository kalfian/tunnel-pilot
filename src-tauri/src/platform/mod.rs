//! Platform integration: dock/taskbar visibility, autostart sync, and
//! notifications (spec 03 §§12,13,15).
//!
//! TODO(M3/M6): fill the submodules below.

pub mod autostart;
pub mod dock;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod notify;
