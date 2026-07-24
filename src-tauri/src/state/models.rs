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
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub launch_at_login: bool,
    pub show_notifications: bool,
    pub theme_mode: ThemeMode,
    pub auto_reconnect: bool,
    pub auto_reconnect_delay_sec: u32,
    pub auto_reconnect_max_retries: u32,
    pub show_in_dock: bool,
    pub auto_check_updates: bool,
    pub last_skipped_version: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            launch_at_login: true,
            show_notifications: true,
            theme_mode: ThemeMode::System,
            auto_reconnect: true,
            auto_reconnect_delay_sec: 5,
            auto_reconnect_max_retries: 3,
            show_in_dock: false,
            auto_check_updates: true,
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
