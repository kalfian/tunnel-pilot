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

/// `()` — the tray popover was opened (tray left-click). Emitted **only** to the
/// `tray_popover` window so its panel UI rehydrates fresh tunnel/group/settings
/// state on every open (the popover webview may have been idle while hidden).
pub const POPOVER_OPENED: &str = "tray://opened";
