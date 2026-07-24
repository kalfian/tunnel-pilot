//! Single shared 3s stats/latency EMIT sampler (reads each tunnel's stats
//! cell, emits `tunnel://stats`). Holds NO session and NEVER tears down —
//! liveness is owned by russh keepalive + the session-future signal (F1/F21).
//!
//! TODO(M2): 3s interval sampler, channel-open latency probe, auto-start/stop.
