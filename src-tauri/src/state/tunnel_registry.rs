//! Per-tunnel runtime registry: `map<TunnelId, TunnelHandle>`.
//!
//! Holds the two-level cancellation token hierarchy (durable `parent_cancel` +
//! per-attempt `attempt_cancel`), the stable `JoinHandle`, the lock-guarded
//! `retry_requested` flag, stats cell, and live status. Status is written ONLY
//! via a guarded `set_status` under the registry lock (F23/F28).
//!
//! TODO(M1): `TunnelHandle`, `set_status` transition guard, token hierarchy.
