//! Sleep/wake watchdog: monotonic-gap detection (>30s ⇒ sweep + immediate
//! reconnect). Best-effort — the session-future signal is the backstop (F15).
//!
//! TODO(M2): monotonic-clock watchdog task; M6 verifies across real OS sleep.
