//! In-memory ring buffer of `LogEntry` (cap 500, newest-first). Not persisted.
//!
//! Backs the Logs tab; fed by the tracing layer + explicit [`LogBuffer::log`]
//! helper, and emits `log://line` / `log://cleared` (spec 03 §18, 04 §6).
//!
//! Storage is a `VecDeque` held newest-first: a push goes to the front and the
//! oldest (back) entry is dropped once the buffer exceeds [`LOG_CAP`], so the
//! snapshot is already in display order (newest first) with no per-read reverse.

use std::collections::VecDeque;
use std::sync::{Mutex, RwLock};

use tauri::{AppHandle, Emitter};

use crate::events;
use crate::state::models::{LogEntry, LogLevel};

/// Maximum number of retained log lines (spec 04 §6). Oldest dropped first.
pub const LOG_CAP: usize = 500;

/// The 500-cap in-memory log ring buffer (spec 03 §18).
///
/// Thread-safe: the entries live behind a short-lived `Mutex` (pushes/reads are
/// cheap and never span an `.await`), and the emit `AppHandle` behind an
/// `RwLock<Option<_>>` because it is only available after Tauri setup — the
/// buffer is constructed at the very start of boot (so the tracing layer can
/// reach it) and the handle is attached once the app exists.
#[derive(Default)]
pub struct LogBuffer {
    entries: Mutex<VecDeque<LogEntry>>,
    app: RwLock<Option<AppHandle>>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach the `AppHandle` used to emit `log://line` / `log://cleared`. Until
    /// this is set (early boot, or headless tests) appends are buffered silently.
    pub fn set_app_handle(&self, app: AppHandle) {
        if let Ok(mut g) = self.app.write() {
            *g = Some(app);
        }
    }

    fn app_handle(&self) -> Option<AppHandle> {
        self.app.read().ok().and_then(|g| g.clone())
    }

    /// Append an already-built entry (newest-first) and emit `log://line`.
    /// Drops the oldest entry when the buffer is full.
    pub fn push(&self, entry: LogEntry) {
        if let Ok(mut q) = self.entries.lock() {
            q.push_front(entry.clone());
            while q.len() > LOG_CAP {
                q.pop_back();
            }
        }
        if let Some(app) = self.app_handle() {
            let _ = app.emit(events::LOG_LINE, entry);
        }
    }

    /// Convenience: build a `LogEntry` from parts (timestamp = now, `HH:mm:ss`)
    /// and append it. Used by the explicit `log()` call sites (spec 03 §18).
    pub fn log(&self, level: LogLevel, tunnel_name: Option<String>, message: impl Into<String>) {
        self.push(LogEntry {
            level,
            tunnel_name,
            message: message.into(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
    }

    /// Snapshot of all retained entries, newest-first (spec 04 §6).
    pub fn snapshot(&self) -> Vec<LogEntry> {
        self.entries
            .lock()
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The joined formatted text for Copy All (`get_logs_text`), newest-first,
    /// one line per entry (spec 04 §6).
    pub fn formatted_text(&self) -> String {
        self.entries
            .lock()
            .map(|q| {
                q.iter()
                    .map(LogEntry::formatted)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default()
    }

    /// Empty the buffer and emit `log://cleared`.
    pub fn clear(&self) {
        if let Ok(mut q) = self.entries.lock() {
            q.clear();
        }
        if let Some(app) = self.app_handle() {
            let _ = app.emit(events::LOG_CLEARED, ());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(msg: &str) -> LogEntry {
        LogEntry {
            level: LogLevel::Info,
            tunnel_name: None,
            message: msg.to_string(),
            timestamp: "00:00:00".to_string(),
        }
    }

    #[test]
    fn buffer_is_newest_first_and_capped() {
        let buf = LogBuffer::new();
        for i in 0..(LOG_CAP + 50) {
            buf.push(entry(&format!("m{i}")));
        }
        let snap = buf.snapshot();
        assert_eq!(snap.len(), LOG_CAP, "buffer never exceeds the cap");
        // Newest-first: the most recent push is at index 0, the oldest survivor last.
        assert_eq!(snap.first().unwrap().message, format!("m{}", LOG_CAP + 49));
        assert_eq!(snap.last().unwrap().message, "m50");
    }

    #[test]
    fn clear_empties_the_buffer() {
        let buf = LogBuffer::new();
        buf.push(entry("a"));
        buf.push(entry("b"));
        assert_eq!(buf.snapshot().len(), 2);
        buf.clear();
        assert!(buf.snapshot().is_empty());
    }

    #[test]
    fn formatted_line_shape_matches_spec() {
        // With a tunnel name: [time] [LEVEL] [tunnel] message.
        let with_tunnel = LogEntry {
            level: LogLevel::Error,
            tunnel_name: Some("Prod DB".into()),
            message: "connection refused".into(),
            timestamp: "12:34:56".into(),
        };
        assert_eq!(
            with_tunnel.formatted(),
            "[12:34:56] [ERROR] [Prod DB] connection refused"
        );
        // App-level (no tunnel): the [tunnel] segment is omitted.
        let app_level = LogEntry {
            level: LogLevel::Info,
            tunnel_name: None,
            message: "started".into(),
            timestamp: "00:00:01".into(),
        };
        assert_eq!(app_level.formatted(), "[00:00:01] [INFO] started");
    }

    #[test]
    fn formatted_text_is_newest_first_joined() {
        let buf = LogBuffer::new();
        buf.push(entry("old"));
        buf.push(entry("new"));
        let text = buf.formatted_text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("new"));
        assert!(lines[1].contains("old"));
    }
}
