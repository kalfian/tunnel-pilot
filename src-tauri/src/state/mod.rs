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
use tokio::sync::mpsc;

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

/// A pending whole-section persistence request. The ordered writer task
/// consumes these; each variant carries the FULL latest snapshot of its section
/// (last-write-wins is correct — every save is a whole-section write).
enum PersistMsg {
    Forwards(Vec<ForwardConfig>),
    Settings(AppSettings),
}

/// The single ordered persistence writer (F37).
///
/// All mutations enqueue the latest full section snapshot on an mpsc channel;
/// this one task is the ONLY writer, so writes land in enqueue order and can
/// never race for the config store's write lock the way detached per-mutation
/// spawns did (an older snapshot overwriting a newer one → silent data loss).
///
/// On each wakeup it coalesces every message already queued down to the latest
/// per section before writing, so a burst of rapid mutations collapses to a
/// single write of the newest state. Because `ConfigStore` does read-merge-write
/// from disk, writing the two sections sequentially preserves siblings; the
/// order between sections within a batch is irrelevant to the final content.
///
/// Persist failures are logged (never with the secret — the config file holds
/// none). Surfacing them to the user is an explicit M4 follow-up: M4 introduces
/// the real command surface, which can offer an async save path returning
/// `Result` (see PROGRESS.md M2 findings).
async fn persist_writer_loop(store: Arc<ConfigStore>, mut rx: mpsc::UnboundedReceiver<PersistMsg>) {
    fn coalesce(
        msg: PersistMsg,
        forwards: &mut Option<Vec<ForwardConfig>>,
        settings: &mut Option<AppSettings>,
    ) {
        match msg {
            PersistMsg::Forwards(v) => *forwards = Some(v),
            PersistMsg::Settings(v) => *settings = Some(v),
        }
    }

    while let Some(first) = rx.recv().await {
        let mut forwards: Option<Vec<ForwardConfig>> = None;
        let mut settings: Option<AppSettings> = None;
        coalesce(first, &mut forwards, &mut settings);
        // Drain anything already queued, keeping only the newest per section.
        while let Ok(msg) = rx.try_recv() {
            coalesce(msg, &mut forwards, &mut settings);
        }

        if let Some(forwards) = forwards {
            if let Err(e) = store.save_forwards(forwards).await {
                tracing::error!(error = %e, "failed to persist forwards");
            }
        }
        if let Some(settings) = settings {
            if let Err(e) = store.save_settings(settings).await {
                tracing::error!(error = %e, "failed to persist settings");
            }
        }
    }
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
    /// Enqueue side of the single ordered persistence writer (F37). `None` in
    /// headless tests (no `config_store` → no writer task). Mutations push the
    /// latest full section snapshot here instead of spawning a detached write.
    persist_tx: Option<mpsc::UnboundedSender<PersistMsg>>,
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
        // Spin up the single ordered persistence writer (F37): one task drains
        // the channel and is the sole writer, so mutations can never race the
        // config store's write lock and land out of order.
        let (persist_tx, persist_rx) = mpsc::unbounded_channel::<PersistMsg>();
        let writer_store = config_store.clone();
        tauri::async_runtime::spawn(persist_writer_loop(writer_store, persist_rx));
        Self {
            registry: Arc::new(TunnelRegistry::new()),
            configs: RwLock::new(doc.forwards),
            groups: RwLock::new(doc.groups),
            settings: RwLock::new(doc.settings),
            credentials,
            keychain_available,
            config_store: Some(config_store),
            persist_tx: Some(persist_tx),
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
            persist_tx: None,
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
    ///
    ///
    /// Synchronous — safe off the async runtime (migration/boot, sync commands).
    /// On the async auth path use [`get_password_async`](Self::get_password_async)
    /// so the blocking keyring call never stalls a tokio worker.
    pub fn get_password(&self, id: &str) -> Option<String> {
        match self.credentials.get_password(id) {
            Ok(pw) => pw,
            Err(e) => {
                tracing::error!(forward_id = %id, error = %e, "failed to read password");
                None
            }
        }
    }

    /// Async variant of [`get_password`](Self::get_password) for the async auth
    /// path (F38). The `keyring` get does blocking OS calls (macOS Security
    /// framework / Linux Secret Service D-Bus) that can stall a tokio worker, so
    /// the read runs on `spawn_blocking` — mirroring the identity-key load in
    /// `ssh/client.rs`. `None` on absence, error, or task panic (logged).
    pub async fn get_password_async(&self, id: &str) -> Option<String> {
        let credentials = self.credentials.clone();
        let account = id.to_string();
        let log_id = id.to_string();
        match tokio::task::spawn_blocking(move || credentials.get_password(&account)).await {
            Ok(Ok(pw)) => pw,
            Ok(Err(e)) => {
                tracing::error!(forward_id = %log_id, error = %e, "failed to read password");
                None
            }
            Err(e) => {
                tracing::error!(forward_id = %log_id, error = %e, "password read task panicked");
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

    // --- persistence helpers (enqueue to the single ordered writer; F37) ---

    /// Enqueue the latest full forwards snapshot for the ordered writer. No-op
    /// when headless (no `persist_tx`). The writer serializes writes in enqueue
    /// order and coalesces bursts to the newest snapshot, so the last mutation
    /// is always what lands on disk (no stale-snapshot data loss).
    fn persist_forwards(&self) {
        if let Some(tx) = &self.persist_tx {
            // Send failure only happens if the writer task is gone (shutdown);
            // logged, never with a secret (the config file holds none).
            if tx
                .send(PersistMsg::Forwards(self.configs_snapshot()))
                .is_err()
            {
                tracing::error!("persistence writer unavailable; forwards not queued");
            }
        }
    }

    /// Enqueue the latest full settings snapshot for the ordered writer (F37).
    fn persist_settings(&self) {
        if let Some(tx) = &self.persist_tx {
            if tx
                .send(PersistMsg::Settings(self.settings_snapshot()))
                .is_err()
            {
                tracing::error!("persistence writer unavailable; settings not queued");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forward_with_port(id: &str, port: u16) -> ForwardConfig {
        ForwardConfig {
            id: id.to_string(),
            name: format!("fwd-{id}"),
            ssh_host: "bastion.example.com".into(),
            ssh_port: 22,
            ssh_username: "deploy".into(),
            identity_file_path: None,
            has_stored_password: false,
            local_bind_address: "127.0.0.1".into(),
            local_port: port,
            remote_host: "db.internal".into(),
            remote_port: 5432,
            keep_alive_interval_sec: 30,
            keep_alive_max_count: 5,
            group_id: None,
            tags: vec![],
        }
    }

    /// F37: a burst of distinct snapshots enqueued rapidly must land on disk as
    /// the LAST enqueued value — never an earlier snapshot written after a newer
    /// one (the data-loss race the detached per-mutation spawns had). The single
    /// ordered writer is the only writer, so enqueue order == write order.
    #[tokio::test]
    async fn ordered_writer_persists_last_enqueued_snapshot() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(ConfigStore::from_config_dir(dir.path()));
        let (tx, rx) = mpsc::unbounded_channel::<PersistMsg>();

        // Burst of 200 distinct forwards snapshots, monotonically newer.
        for i in 0..200u16 {
            let snapshot = vec![forward_with_port("f", 1000 + i)];
            tx.send(PersistMsg::Forwards(snapshot)).expect("enqueue");
        }
        // The final, newest snapshot.
        let last = vec![forward_with_port("f", 9999), forward_with_port("g", 8888)];
        tx.send(PersistMsg::Forwards(last.clone()))
            .expect("enqueue last");

        // Close the channel so the writer drains and returns, then run it to
        // completion (drives the same loop production uses).
        drop(tx);
        persist_writer_loop(store.clone(), rx).await;

        // The on-disk forwards equal the LAST enqueued snapshot, not an earlier one.
        let doc = store.load().await.expect("load");
        assert_eq!(doc.forwards, last);
    }

    /// The writer coalesces mixed-section bursts to the newest of EACH section,
    /// and both land (read-merge-write preserves the sibling).
    #[tokio::test]
    async fn ordered_writer_coalesces_both_sections_to_latest() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(ConfigStore::from_config_dir(dir.path()));
        let (tx, rx) = mpsc::unbounded_channel::<PersistMsg>();

        for i in 0..50u16 {
            tx.send(PersistMsg::Forwards(vec![forward_with_port("f", 2000 + i)]))
                .expect("enqueue fwd");
            let settings = AppSettings {
                auto_reconnect_max_retries: i as u32,
                ..AppSettings::default()
            };
            tx.send(PersistMsg::Settings(settings))
                .expect("enqueue settings");
        }
        let last_forwards = vec![forward_with_port("final", 7777)];
        let last_settings = AppSettings {
            auto_reconnect_max_retries: 99,
            show_in_dock: true,
            ..AppSettings::default()
        };
        tx.send(PersistMsg::Forwards(last_forwards.clone()))
            .expect("enqueue last fwd");
        tx.send(PersistMsg::Settings(last_settings.clone()))
            .expect("enqueue last settings");

        drop(tx);
        persist_writer_loop(store.clone(), rx).await;

        let doc = store.load().await.expect("load");
        assert_eq!(doc.forwards, last_forwards);
        assert_eq!(doc.settings, last_settings);
    }
}
