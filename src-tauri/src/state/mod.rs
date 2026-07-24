//! Application state — the source of truth for the app (spec 02 §5).
//!
//! `AppState` is wired as `tauri::State` and owns the tunnel registry plus (for
//! M1) in-memory mirrors of the config list, settings, and credentials. The
//! persisted config file (M2 `storage/`) and the OS keychain (M2
//! `credentials/`) replace the in-memory `configs`/`passwords` maps later; the
//! engine only ever reads through `AppState`, so that swap is transparent.

pub mod log_buffer;
pub mod models;
pub mod settings_state;
pub mod tunnel_registry;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tauri::Emitter;

use crate::events;
use crate::state::models::{AppSettings, ForwardConfig, ForwardStatus, TunnelId, TunnelStats};
use crate::state::tunnel_registry::TunnelRegistry;

/// Payload for `tunnel://status` (spec 02 §7, events.rs).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusPayload {
    pub id: TunnelId,
    pub status: ForwardStatus,
    pub last_error: Option<String>,
}

/// Payload for `tunnel://stats` (spec 02 §7, events.rs).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsPayload {
    pub id: TunnelId,
    pub stats: TunnelStats,
}

/// The shared application state (`Arc<AppState>`), managed by Tauri and cloned
/// into each supervisor task.
pub struct AppState {
    pub registry: Arc<TunnelRegistry>,
    /// Config list (M1: in-memory; M2: mirror of the persisted file).
    pub configs: RwLock<HashMap<TunnelId, ForwardConfig>>,
    /// App settings (M1: in-memory defaults; M2: persisted mirror).
    pub settings: RwLock<AppSettings>,
    /// Credential stand-in (M1: in-memory; M2: OS keychain / fallback file).
    passwords: RwLock<HashMap<TunnelId, String>>,
    /// `AppHandle` for emitting events; `None` in headless engine tests.
    app: RwLock<Option<tauri::AppHandle>>,
}

impl AppState {
    /// Construct with a live Tauri `AppHandle` (production).
    pub fn new(app: tauri::AppHandle) -> Self {
        Self {
            registry: Arc::new(TunnelRegistry::new()),
            configs: RwLock::new(HashMap::new()),
            settings: RwLock::new(AppSettings::default()),
            passwords: RwLock::new(HashMap::new()),
            app: RwLock::new(Some(app)),
        }
    }

    /// Construct without an `AppHandle` — for engine tests. Event emission is a
    /// no-op; everything else behaves identically.
    pub fn new_headless() -> Self {
        Self {
            registry: Arc::new(TunnelRegistry::new()),
            configs: RwLock::new(HashMap::new()),
            settings: RwLock::new(AppSettings::default()),
            passwords: RwLock::new(HashMap::new()),
            app: RwLock::new(None),
        }
    }

    fn app_handle(&self) -> Option<tauri::AppHandle> {
        self.app.read().ok().and_then(|g| g.clone())
    }

    /// Emit `tunnel://status` (no-op when headless).
    pub fn emit_status(&self, id: &str, status: ForwardStatus, last_error: Option<String>) {
        if let Some(app) = self.app_handle() {
            let _ = app.emit(
                events::TUNNEL_STATUS,
                StatusPayload {
                    id: id.to_string(),
                    status,
                    last_error,
                },
            );
        }
    }

    /// Emit `tunnel://stats` (no-op when headless).
    pub fn emit_stats(&self, id: &str, stats: TunnelStats) {
        if let Some(app) = self.app_handle() {
            let _ = app.emit(
                events::TUNNEL_STATS,
                StatsPayload {
                    id: id.to_string(),
                    stats,
                },
            );
        }
    }

    // --- config / settings / credential accessors (M1 in-memory) ---

    pub fn get_config(&self, id: &str) -> Option<ForwardConfig> {
        self.configs.read().ok().and_then(|g| g.get(id).cloned())
    }

    pub fn upsert_config(&self, config: ForwardConfig) {
        if let Ok(mut g) = self.configs.write() {
            g.insert(config.id.clone(), config);
        }
    }

    pub fn settings_snapshot(&self) -> AppSettings {
        self.settings.read().map(|g| g.clone()).unwrap_or_default()
    }

    pub fn set_password(&self, id: &str, pw: String) {
        if let Ok(mut g) = self.passwords.write() {
            g.insert(id.to_string(), pw);
        }
    }

    pub fn get_password(&self, id: &str) -> Option<String> {
        self.passwords.read().ok().and_then(|g| g.get(id).cloned())
    }
}
