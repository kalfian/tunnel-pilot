//! Wire/domain models shared across IPC and the engine (spec 04).
//!
//! All serde structs use `#[serde(rename_all = "camelCase")]` so the JSON wire
//! format matches the TypeScript types 1:1 (AGENTS.md §1, spec 04 Conventions).
//! Only the models needed by the M1 SSH engine live here; the remaining models
//! (groups, logs, backup, update) are added with their subsystems (M2/M4/M6).

use serde::{Deserialize, Serialize};

/// A tunnel identifier (uuid v4) — spec 03 Conventions.
pub type TunnelId = String;

/// SSH local-forward configuration (spec 04 §1).
///
/// The plaintext password is NOT part of this model — secrets live in the
/// keychain/fallback store (spec 03 §9). `#[serde(default)]` on the v2-only and
/// runtime-normalized fields lets a lenient v1 backup entry parse (F19, spec 04 §11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardConfig {
    pub id: String,
    pub name: String,
    pub ssh_host: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub ssh_username: String,
    #[serde(default)]
    pub identity_file_path: Option<String>,
    #[serde(default)]
    pub has_stored_password: bool,
    #[serde(default = "default_bind_address")]
    pub local_bind_address: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    #[serde(default = "default_keepalive_interval")]
    pub keep_alive_interval_sec: u32,
    #[serde(default = "default_keepalive_max")]
    pub keep_alive_max_count: u32,
    #[serde(default)]
    pub group_id: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_ssh_port() -> u16 {
    22
}
fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}
fn default_keepalive_interval() -> u32 {
    30
}
fn default_keepalive_max() -> u32 {
    5
}

impl ForwardConfig {
    /// Effective keepalive interval in seconds (spec 03 §2): `0 → 10` for faster
    /// VPN-death detection; otherwise the configured value.
    pub fn effective_keepalive_interval_sec(&self) -> u64 {
        if self.keep_alive_interval_sec == 0 {
            10
        } else {
            self.keep_alive_interval_sec as u64
        }
    }

    /// Effective keepalive max-count (spec 03 §2): `0 → 3`; otherwise the
    /// configured value. Maps directly to `russh` `client::Config.keepalive_max`.
    pub fn effective_keepalive_max(&self) -> usize {
        if self.keep_alive_max_count == 0 {
            3
        } else {
            self.keep_alive_max_count as usize
        }
    }

    /// Derived (not persisted): auth needs a password because there is no
    /// identity file and no stored secret (spec 04 §1).
    pub fn needs_password(&self) -> bool {
        self.identity_file_path.is_none() && !self.has_stored_password
    }
}

/// A folder that forwards can belong to (new in v2, spec 04 §2). A forward has
/// at most one exclusive `group_id`; ungrouped forwards render under a default
/// section. `collapsed` is persisted (F13) so folder state survives restarts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    pub order: u32,
    #[serde(default)]
    pub collapsed: bool,
}

/// Create/update payload for a group (spec 04 §2) — no `id`, no `order`
/// (assigned by the backend on create; preserved on update).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInput {
    pub name: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
}

/// Create/update payload — no `id`, no live state, no secret (spec 04 §1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInput {
    pub name: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_username: String,
    pub identity_file_path: Option<String>,
    pub local_bind_address: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub keep_alive_interval_sec: u32,
    pub keep_alive_max_count: u32,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
}

/// The 5-state tunnel status (spec 04 §4). `disconnecting` is a real transient
/// (clicks ignored while in it); `error` allows retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardStatus {
    Disconnected,
    Connecting,
    Connected,
    Disconnecting,
    Error,
}

/// Per-tunnel stats snapshot emitted on `tunnel://stats` (spec 04 §5).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStats {
    pub active_connections: usize,
    pub total_bytes_up: u64,
    pub total_bytes_down: u64,
    pub last_ping_latency_ms: Option<u64>,
    pub connected_since: Option<String>,
}

/// Combined runtime view returned by `get_forward_runtime` / in `AppSnapshot`
/// (spec 04 §5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardRuntime {
    pub status: ForwardStatus,
    pub stats: TunnelStats,
    pub last_error: Option<String>,
}

/// Application settings (spec 04 §3). Only the fields the M1 engine reads
/// (`auto_reconnect`, delay, max-retries) are load-bearing here; the rest are
/// carried for the persisted mirror (M2).
///
/// Every field carries `#[serde(default)]` (with the correct v1 default) so a
/// partial or legacy settings block MERGES with defaults field-by-field: a file
/// missing one key keeps every other configured value instead of resetting the
/// whole struct to `Default`. The defaults below MUST stay in lockstep with the
/// [`Default`] impl (both delegate to the same free functions).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub launch_at_login: bool,
    #[serde(default = "default_true")]
    pub show_notifications: bool,
    #[serde(default = "default_theme_mode")]
    pub theme_mode: ThemeMode,
    #[serde(default = "default_true")]
    pub auto_reconnect: bool,
    #[serde(default = "default_reconnect_delay_sec")]
    pub auto_reconnect_delay_sec: u32,
    #[serde(default = "default_reconnect_max_retries")]
    pub auto_reconnect_max_retries: u32,
    #[serde(default)]
    pub show_in_dock: bool,
    #[serde(default = "default_true")]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub last_skipped_version: Option<String>,
}

fn default_true() -> bool {
    true
}
fn default_theme_mode() -> ThemeMode {
    ThemeMode::System
}
fn default_reconnect_delay_sec() -> u32 {
    5
}
fn default_reconnect_max_retries() -> u32 {
    3
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            launch_at_login: default_true(),
            show_notifications: default_true(),
            theme_mode: default_theme_mode(),
            auto_reconnect: default_true(),
            auto_reconnect_delay_sec: default_reconnect_delay_sec(),
            auto_reconnect_max_retries: default_reconnect_max_retries(),
            show_in_dock: false,
            auto_check_updates: default_true(),
            last_skipped_version: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

/// A single in-memory log line (spec 04 §6). Not persisted; the ring buffer is
/// capped at 500 newest-first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub level: LogLevel,
    /// `None` for app-level (non-tunnel) logs.
    pub tunnel_name: Option<String>,
    pub message: String,
    /// Formatted local time `HH:mm:ss`.
    pub timestamp: String,
}

impl LogEntry {
    /// The Copy-All line format (spec 04 §6): `[HH:mm:ss] [LEVEL] [tunnel] message`
    /// — the `[tunnel]` segment is omitted when there is no tunnel name.
    pub fn formatted(&self) -> String {
        let level = match self.level {
            LogLevel::Info => "INFO",
            LogLevel::Warning => "WARNING",
            LogLevel::Error => "ERROR",
        };
        match &self.tunnel_name {
            Some(t) => format!("[{}] [{}] [{}] {}", self.timestamp, level, t, self.message),
            None => format!("[{}] [{}] {}", self.timestamp, level, self.message),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

/// Self-update availability snapshot (spec 04 §7). Returned by `check_update`
/// and carried in [`AppSnapshot`]; the real check is wired in M6.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    /// `version == settings.last_skipped_version`.
    pub skipped: bool,
}

/// One-shot boot/rehydrate snapshot returned by `app_hydrate` (spec 04 §8). The
/// frontend fully rehydrates from this on window show — it holds no
/// authoritative state of its own (spec 02 §5).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub forwards: Vec<ForwardConfig>,
    pub groups: Vec<TunnelGroup>,
    pub settings: AppSettings,
    pub logs: Vec<LogEntry>,
    /// `(forwardId, runtime)` pairs for every currently-live tunnel.
    pub runtimes: Vec<(String, ForwardRuntime)>,
    pub update: UpdateStatus,
    pub keychain_available: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_settings_partial_block_merges_with_defaults() {
        // A legacy/partial settings block sets ONE field; every other field must
        // fall back to its v1 default rather than the whole struct resetting.
        let json = r#"{ "showInDock": true }"#;
        let parsed: AppSettings = serde_json::from_str(json).expect("parse partial settings");

        // The one present field is honored...
        assert!(parsed.show_in_dock);
        // ...and the rest match the documented v1 defaults (NOT bool/int zero).
        let defaults = AppSettings::default();
        assert_eq!(parsed.launch_at_login, defaults.launch_at_login);
        assert_eq!(parsed.show_notifications, defaults.show_notifications);
        assert_eq!(parsed.theme_mode, defaults.theme_mode);
        assert_eq!(parsed.auto_reconnect, defaults.auto_reconnect);
        assert_eq!(
            parsed.auto_reconnect_delay_sec,
            defaults.auto_reconnect_delay_sec
        );
        assert_eq!(
            parsed.auto_reconnect_max_retries,
            defaults.auto_reconnect_max_retries
        );
        assert_eq!(parsed.auto_check_updates, defaults.auto_check_updates);
        assert_eq!(parsed.last_skipped_version, defaults.last_skipped_version);
    }

    #[test]
    fn app_settings_empty_block_equals_default() {
        let parsed: AppSettings = serde_json::from_str("{}").expect("parse empty settings");
        assert_eq!(parsed, AppSettings::default());
    }
}
