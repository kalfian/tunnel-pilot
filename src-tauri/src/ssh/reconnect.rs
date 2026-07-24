//! `backoff()` helper (exponential, clamp 1–60s). The reconnect loop itself
//! lives in the `engine.rs` supervisor (F21); this module is the pure helper.
//!
//! TODO(M2): `backoff(attempt) -> Duration` + unit tests.
