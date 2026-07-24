//! `TunnelStats` accounting: atomic byte/connection counters (`StatsInner`)
//! converted to the `TunnelStats` wire model for IPC (spec 03 §6, 04 §5).
//!
//! TODO(M1): `StatsInner` atomics + snapshot conversion.
