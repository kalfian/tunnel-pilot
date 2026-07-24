//! SSH tunnel engine (russh 0.45, pinned in M1).
//!
//! One long-lived supervisor task per tunnel owns its session in-task and loops
//! across reconnect attempts; liveness is owned by russh keepalive + the
//! session-future signal (no app-level ping). See spec 03 §§1,2,5,6.
//!
//! TODO(M1/M2): fill the submodules below.

pub mod client;
pub mod engine;
pub mod forward;
pub mod health;
pub mod reconnect;
pub mod stats;
pub mod wake;

/// In-process russh integration tests (spec 03 acceptance). See the module doc.
#[cfg(test)]
mod it_tests;
