//! Tracing initialization + the tracing→log-buffer bridge (spec 03 §18).
//!
//! A real `tracing_subscriber` registry (env filter + fmt layer) plus the
//! [`LogBufferLayer`], which formats each of OUR crate's `INFO`/`WARN`/`ERROR`
//! events into a [`LogEntry`], pushes it into the 500-cap ring buffer
//! (`state/log_buffer.rs`), and emits `log://line`.
//!
//! The buffer is a process-global `Arc<LogBuffer>` set via [`set_log_buffer`]
//! before `init_tracing` runs, so the layer (installed globally, no access to
//! `AppState`) can reach it. `lib.rs` shares the SAME `Arc` with `AppState` so
//! the `get_logs` command reads the exact buffer the layer writes.

use std::sync::{Arc, OnceLock};

use tracing::field::{Field, Visit};
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{EnvFilter, Layer};

use crate::state::log_buffer::LogBuffer;
use crate::state::models::{LogEntry, LogLevel};

/// Only events from our own crate reach the user-facing Logs tab; third-party
/// `INFO` lines (tao/wry/russh) are noise the user should not see.
const CRATE_TARGET_PREFIX: &str = "tunnel_pilot_lib";

/// Process-global log buffer the tracing layer writes into. Set once at boot
/// (before `init_tracing`) and shared with `AppState` so reads and writes hit
/// the same ring buffer.
static LOG_BUFFER: OnceLock<Arc<LogBuffer>> = OnceLock::new();

/// Install the shared log buffer for the tracing layer. Idempotent — a second
/// call (e.g. in tests) is ignored.
pub fn set_log_buffer(buffer: Arc<LogBuffer>) {
    let _ = LOG_BUFFER.set(buffer);
}

/// Extracts the `message` and an optional `tunnel`/`tunnel_name` field from a
/// tracing event's fields (the engine logs `tracing::info!(tunnel = %id, "...")`).
#[derive(Default)]
struct FieldExtractor {
    message: Option<String>,
    tunnel: Option<String>,
}

impl Visit for FieldExtractor {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message = Some(value.to_string()),
            "tunnel" | "tunnel_name" => self.tunnel = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let rendered = format!("{value:?}");
        match field.name() {
            "message" => self.message = Some(rendered),
            "tunnel" | "tunnel_name" => {
                // A `%id` / `?id` field renders quoted for strings via Debug;
                // strip the surrounding quotes so the tunnel name reads cleanly.
                self.tunnel = Some(rendered.trim_matches('"').to_string());
            }
            _ => {}
        }
    }
}

/// A `tracing` layer that forwards our crate's user-visible events into the
/// in-memory log ring buffer and emits `log://line`.
#[derive(Default)]
pub struct LogBufferLayer;

impl LogBufferLayer {
    pub fn new() -> Self {
        Self
    }
}

impl<S: tracing::Subscriber> Layer<S> for LogBufferLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        let Some(buffer) = LOG_BUFFER.get() else {
            return; // buffer not installed yet (very early boot) — skip.
        };
        let meta = event.metadata();

        // Only surface our own crate's events (skip noisy third-party logs).
        if !meta.target().starts_with(CRATE_TARGET_PREFIX) {
            return;
        }

        // Map the tracing level to the user-facing LogLevel; DEBUG/TRACE never
        // reach the Logs tab.
        let level = match *meta.level() {
            tracing::Level::ERROR => LogLevel::Error,
            tracing::Level::WARN => LogLevel::Warning,
            tracing::Level::INFO => LogLevel::Info,
            _ => return,
        };

        let mut extractor = FieldExtractor::default();
        event.record(&mut extractor);
        let Some(message) = extractor.message else {
            return; // no human message — nothing useful to show.
        };

        buffer.push(LogEntry {
            level,
            tunnel_name: extractor.tunnel,
            message,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
    }
}

/// Initialize the global tracing subscriber. Idempotent-safe: uses `try_init`
/// so a second call (e.g. in tests) is a no-op rather than a panic.
///
/// # `log` → tracing bridge (russh diagnostics)
///
/// russh 0.45 and russh-keys 0.45 emit through the `log` crate, NOT `tracing`,
/// so without a bridge their handshake/KEX/algorithm-negotiation records never
/// reach this subscriber. [`tracing_log::LogTracer`] captures every `log` record
/// and forwards it into `tracing`, where the [`EnvFilter`] below decides what is
/// actually shown. LogTracer leaves the `log` max level at `Trace`, so `RUST_LOG`
/// alone controls verbosity — nothing is filtered out before it reaches us.
///
/// # Filter
///
/// `RUST_LOG` is honored when set; otherwise a quiet default of
/// `tunnel_pilot_lib=info` keeps third-party crates (tao/wry/russh) silent so the
/// dev log is not spammed. To capture the "Unknown algorithm" negotiation, run
/// with `RUST_LOG=tunnel_pilot_lib=debug,russh=debug,russh_keys=debug`.
pub fn init_tracing() {
    // Bridge `log` → tracing so russh's negotiation logs are capturable. `.ok()`
    // (via `let _`) makes a repeat call, or a pre-existing `log` logger, a no-op
    // instead of an error.
    let _ = tracing_log::LogTracer::init();

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("tunnel_pilot_lib=info"));

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(LogBufferLayer::new())
        .try_init();
}
