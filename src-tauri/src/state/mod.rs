//! Application state — the source of truth for the app (spec 02 §5).
//!
//! `AppState` is wired as `tauri::State`. It owns the tunnel registry plus the
//! RAM mirrors of the persisted config (forwards / groups / settings) and the
//! [`CredentialStore`]. The engine only ever reads config through `get_config`
//! and passwords through `get_password`, so where those come from is an
//! `AppState` implementation detail: since M2 they are backed by the persisted
//! `tunnel_pilot_config.json` ([`ConfigStore`]) and the OS keychain /
//! fallback secrets file ([`CredentialStore`]) rather than M1's in-memory maps.
//! Mutations update the RAM mirror synchronously and are flushed to disk.

pub mod log_buffer;
pub mod models;
pub mod settings_state;
pub mod tunnel_registry;

use std::sync::{Arc, RwLock};

use serde::Serialize;
use tauri::Emitter;

use crate::credentials::CredentialStore;
use crate::events;
use crate::state::models::{
    AppSettings, ForwardConfig, ForwardStatus, TunnelGroup, TunnelId, TunnelStats,
};
use crate::state::tunnel_registry::TunnelRegistry;
use crate::storage::config_file::{ConfigDocument, ConfigStore};

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

/// Read-only startup snapshot for the frontend (the real `app_hydrate` command
/// lands in M4; this makes the data reachable now). `keychain_available` drives
/// the persistent "passwords stored in plaintext" UI warning (spec 04 §10).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HydrateSnapshot {
    pub forwards: Vec<ForwardConfig>,
    pub groups: Vec<TunnelGroup>,
    pub settings: AppSettings,
    pub keychain_available: bool,
}

/// The shared application state (`Arc<AppState>`), managed by Tauri and cloned
/// into each supervisor task.
pub struct AppState {
    pub registry: Arc<TunnelRegistry>,
    /// Config list — RAM mirror of the persisted file. A `Vec` (not a map) so
    /// the array order = display order (spec 04 §9); lookups are a linear scan
    /// over the handful of configured forwards (not a hot path).
    configs: RwLock<Vec<ForwardConfig>>,
    /// Groups — RAM mirror of the persisted file (spec 04 §2).
    groups: RwLock<Vec<TunnelGroup>>,
    /// App settings — RAM mirror of the persisted file.
    settings: RwLock<AppSettings>,
    /// Password store: OS keychain first, plaintext fallback file second.
    credentials: Arc<CredentialStore>,
    /// Cached keychain availability (propagated to the UI warning).
    keychain_available: bool,
    /// Persistence handle. `None` in headless engine tests (nothing is flushed
    /// to disk); `Some` in production (mutations are written back).
    config_store: Option<Arc<ConfigStore>>,
    /// `AppHandle` for emitting events; `None` in headless engine tests.
    app: RwLock<Option<tauri::AppHandle>>,
}

impl AppState {
    /// Production constructor: build from the loaded config document plus the
    /// persistence + credential stores. The RAM mirrors are seeded from `doc`.
    pub fn new_hydrated(
        app: tauri::AppHandle,
        config_store: Arc<ConfigStore>,
        credentials: Arc<CredentialStore>,
        doc: ConfigDocument,
    ) -> Self {
        let keychain_available = credentials.keychain_available();
        Self {
            registry: Arc::new(TunnelRegistry::new()),
            configs: RwLock::new(doc.forwards),
            groups: RwLock::new(doc.groups),
            settings: RwLock::new(doc.settings),
            credentials,
            keychain_available,
            config_store: Some(config_store),
            app: RwLock::new(Some(app)),
        }
    }

    /// Construct without an `AppHandle`, persistence, or a real keychain — for
    /// engine tests. Event emission is a no-op, nothing is flushed to disk, and
    /// credentials live in an in-memory store. Everything else behaves
    /// identically.
    pub fn new_headless() -> Self {
        let credentials = Arc::new(CredentialStore::in_memory());
        let keychain_available = credentials.keychain_available();
        Self {
            registry: Arc::new(TunnelRegistry::new()),
            configs: RwLock::new(Vec::new()),
            groups: RwLock::new(Vec::new()),
            settings: RwLock::new(AppSettings::default()),
            credentials,
            keychain_available,
            config_store: None,
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

    // --- config accessors (RAM mirror, persisted on mutation) ---

    /// Look up a config by id (linear scan over the config list).
    pub fn get_config(&self, id: &str) -> Option<ForwardConfig> {
        self.configs
            .read()
            .ok()
            .and_then(|g| g.iter().find(|c| c.id == id).cloned())
    }

    /// Insert or replace a config (preserving display position on replace) and
    /// flush the full forwards list to disk (best-effort, off the caller path).
    pub fn upsert_config(&self, config: ForwardConfig) {
        if let Ok(mut g) = self.configs.write() {
            match g.iter_mut().find(|c| c.id == config.id) {
                Some(existing) => *existing = config,
                None => g.push(config),
            }
        }
        self.persist_forwards();
    }

    /// Remove a config by id and flush. Returns whether it existed.
    pub fn remove_config(&self, id: &str) -> bool {
        let removed = if let Ok(mut g) = self.configs.write() {
            let before = g.len();
            g.retain(|c| c.id != id);
            g.len() != before
        } else {
            false
        };
        if removed {
            self.persist_forwards();
        }
        removed
    }

    /// Snapshot of all configs in display order.
    pub fn configs_snapshot(&self) -> Vec<ForwardConfig> {
        self.configs.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Snapshot of all groups.
    pub fn groups_snapshot(&self) -> Vec<TunnelGroup> {
        self.groups.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Current settings.
    pub fn settings_snapshot(&self) -> AppSettings {
        self.settings.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Replace settings and flush to disk (best-effort).
    pub fn set_settings(&self, settings: AppSettings) {
        if let Ok(mut g) = self.settings.write() {
            *g = settings;
        }
        self.persist_settings();
    }

    /// Whether the OS keychain is usable (false → plaintext fallback in use →
    /// the UI shows a persistent warning).
    pub fn keychain_available(&self) -> bool {
        self.keychain_available
    }

    /// Read-only startup snapshot for the frontend (spec 04 §10; M4 wires the
    /// actual `app_hydrate` command).
    pub fn hydrate_snapshot(&self) -> HydrateSnapshot {
        HydrateSnapshot {
            forwards: self.configs_snapshot(),
            groups: self.groups_snapshot(),
            settings: self.settings_snapshot(),
            keychain_available: self.keychain_available,
        }
    }

    // --- credential accessors (keychain / fallback file) ---

    /// Store the password for a forward. Errors are logged (never surfaced with
    /// the secret) and swallowed to keep the accessor infallible for callers;
    /// the real command surface (M4) reports failures to the user.
    pub fn set_password(&self, id: &str, pw: String) {
        if let Err(e) = self.credentials.set_password(id, &pw) {
            tracing::error!(forward_id = %id, error = %e, "failed to store password");
        }
    }

    /// Fetch the password for a forward, if stored. `None` on absence or error
    /// (the error is logged; the engine treats a missing password as an auth
    /// precondition failure).
    pub fn get_password(&self, id: &str) -> Option<String> {
        match self.credentials.get_password(id) {
            Ok(pw) => pw,
            Err(e) => {
                tracing::error!(forward_id = %id, error = %e, "failed to read password");
                None
            }
        }
    }

    /// Delete a forward's password from every store (best-effort).
    pub fn delete_password(&self, id: &str) {
        if let Err(e) = self.credentials.delete_password(id) {
            tracing::error!(forward_id = %id, error = %e, "failed to delete password");
        }
    }

    // --- persistence helpers (fire-and-forget; None store = no-op) ---

    fn persist_forwards(&self) {
        if let Some(store) = &self.config_store {
            let store = store.clone();
            let forwards = self.configs_snapshot();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = store.save_forwards(forwards).await {
                    tracing::error!(error = %e, "failed to persist forwards");
                }
            });
        }
    }

    fn persist_settings(&self) {
        if let Some(store) = &self.config_store {
            let store = store.clone();
            let settings = self.settings_snapshot();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = store.save_settings(settings).await {
                    tracing::error!(error = %e, "failed to persist settings");
                }
            });
        }
    }
}
