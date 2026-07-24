//! Long-lived per-tunnel supervisor: connect/reconnect loop owning the session,
//! cancellation-aware bind→connect→auth, 5×500ms EADDRINUSE bind-retry, and the
//! guarded `set_status` authority (F21/F23/F24/F25).
//!
//! TODO(M1): supervisor task, conflict handling, session-future = lost.
