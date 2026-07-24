//! Tracing initialization + the tracing→log-buffer bridge (spec 03 §18).
//!
//! M0 wires a real `tracing_subscriber` registry (env filter + fmt layer) plus
//! a **stub** `LogBufferLayer`. M2 fleshes the layer out to format each event
//! into a `LogEntry`, push it into the 500-cap ring buffer
//! (`state/log_buffer.rs`), and emit `log://line`.

use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Layer};

/// A `tracing` layer that will forward user-visible events into the in-memory
/// log ring buffer. **Stub for M0** — currently a no-op.
#[derive(Default)]
pub struct LogBufferLayer;

impl LogBufferLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S: tracing::Subscriber> Layer<S> for LogBufferLayer {
    fn on_event(&self, _event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        // TODO(M2): format into a LogEntry (level/tunnel/message/HH:mm:ss),
        // push onto the 500-cap ring buffer, and emit `log://line`.
    }
}

/// Initialize the global tracing subscriber. Idempotent-safe: uses `try_init`
/// so a second call (e.g. in tests) is a no-op rather than a panic.
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(LogBufferLayer::new())
        .try_init();
}
