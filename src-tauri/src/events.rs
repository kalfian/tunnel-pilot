//! Event-name constants for Rust→Frontend events (spec 02 §7).
//!
//! Emitted via `AppHandle::emit`; the frontend subscribes to these exact
//! strings in `src/lib/events.ts`. Never emit an ad-hoc name not listed here
//! (AGENTS.md §1). Payload structs are added alongside the emitting subsystem
//! in later milestones; the wire shape for each is documented per constant.

/// `{ id: String, status: ForwardStatus, lastError: Option<String> }` — a
/// tunnel status transition.
pub const TUNNEL_STATUS: &str = "tunnel://status";

/// `{ id: String, stats: TunnelStats }` — stats update on the single 3s
/// sampler tick.
pub const TUNNEL_STATS: &str = "tunnel://stats";

/// `LogEntry` — a new log line was appended to the ring buffer.
pub const LOG_LINE: &str = "log://line";

/// `()` — the log buffer was cleared.
pub const LOG_CLEARED: &str = "log://cleared";

/// `Vec<ForwardConfig>` — the config list mutated (CRUD/reorder/migration).
pub const FORWARDS_CHANGED: &str = "forwards://changed";

/// `Vec<TunnelGroup>` — groups mutated.
pub const GROUPS_CHANGED: &str = "groups://changed";

/// `AppSettings` — settings changed (e.g. theme applied elsewhere).
pub const SETTINGS_CHANGED: &str = "settings://changed";

/// `UpdateStatus` — update availability changed.
pub const UPDATE_STATUS: &str = "update://status";

/// `{ downloaded: u64, total: Option<u64> }` — update download progress.
pub const UPDATE_PROGRESS: &str = "update://progress";

/// `()` — the window was re-shown (e.g. via single-instance); the frontend may
/// refresh.
pub const WINDOW_FOCUS: &str = "window://focus";

/// One event as the web transport sends it (spec 02 §10): `{ event, payload }`,
/// with the same name and payload the desktop webview receives via Tauri.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BusEvent {
    pub event: &'static str,
    pub payload: serde_json::Value,
}

/// Process-wide fan-out of §7 events to non-Tauri listeners (the web UI's
/// WebSocket clients). Cheap when nobody listens: the payload is only
/// serialized while at least one subscriber exists. A slow subscriber lags and
/// drops events (it resyncs via `app_hydrate`), so emitters never block.
#[derive(Clone)]
pub struct EventBus {
    tx: tokio::sync::broadcast::Sender<BusEvent>,
}

/// Buffered events per subscriber before it is considered lagged.
const BUS_CAPACITY: usize = 512;

impl Default for EventBus {
    fn default() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(BUS_CAPACITY);
        Self { tx }
    }
}

impl EventBus {
    /// Publish `payload` under `event` if anyone is listening.
    pub fn publish<P: serde::Serialize>(&self, event: &'static str, payload: &P) {
        if self.tx.receiver_count() == 0 {
            return;
        }
        match serde_json::to_value(payload) {
            Ok(payload) => {
                let _ = self.tx.send(BusEvent { event, payload });
            }
            Err(e) => tracing::warn!(event, error = %e, "event payload not serializable"),
        }
    }

    /// A new receiver that sees every event published from now on.
    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<BusEvent> {
        self.tx.subscribe()
    }
}
