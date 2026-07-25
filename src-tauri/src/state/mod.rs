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

use tauri::Emitter;
use tokio::sync::{mpsc, oneshot};

use crate::credentials::CredentialStore;
use crate::error::AppError;
use crate::events;
use crate::state::log_buffer::LogBuffer;
use crate::state::models::{
    AppSettings, AppSnapshot, ForwardConfig, ForwardStatus, TunnelGroup, TunnelId, TunnelStats,
    UpdateStatus,
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

/// The full section snapshot carried by a [`PersistMsg`]. Each variant is the
/// latest whole-section state (last-write-wins — every save is a whole-section
/// write).
enum Section {
    Forwards(Vec<ForwardConfig>),
    Settings(AppSettings),
    Groups(Vec<TunnelGroup>),
}

/// A pending persistence request for the single ordered writer. `ack` (when
/// present) is signalled with the ACTUAL write result after the section lands
/// on disk, so a command that mutated state can surface a persist failure to the
/// user (F37 M4 follow-up). A `None` ack is a fire-and-forget write.
struct PersistMsg {
    section: Section,
    ack: Option<oneshot::Sender<Result<(), AppError>>>,
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
/// from disk, writing the sections sequentially preserves siblings; the order
/// between sections within a batch is irrelevant to the final content.
///
/// Error surfacing (F37 M4 follow-up): the write result for each section is sent
/// back to EVERY ack collected for that section in the batch. When mutations
/// coalesce, they all wanted their-or-newer state persisted, so the single
/// write's result is the correct answer for all of them. Failures are also
/// logged (never with a secret — the config file holds none). The ordering
/// guarantee is untouched: one writer, enqueue order preserved.
async fn persist_writer_loop(store: Arc<ConfigStore>, mut rx: mpsc::UnboundedReceiver<PersistMsg>) {
    /// Per-section coalescing accumulator: newest payload + all acks awaiting it.
    #[derive(Default)]
    struct Pending<T> {
        payload: Option<T>,
        acks: Vec<oneshot::Sender<Result<(), AppError>>>,
    }
    impl<T> Pending<T> {
        fn accept(&mut self, payload: T, ack: Option<oneshot::Sender<Result<(), AppError>>>) {
            self.payload = Some(payload);
            if let Some(ack) = ack {
                self.acks.push(ack);
            }
        }
        /// Send `result` to every awaiting ack (dropped receivers are ignored).
        fn resolve(self, result: &Result<(), AppError>) {
            for ack in self.acks {
                let _ = ack.send(result.clone());
            }
        }
    }

    while let Some(first) = rx.recv().await {
        let mut forwards: Pending<Vec<ForwardConfig>> = Pending::default();
        let mut settings: Pending<AppSettings> = Pending::default();
        let mut groups: Pending<Vec<TunnelGroup>> = Pending::default();

        let mut accept = |msg: PersistMsg| match msg.section {
            Section::Forwards(v) => forwards.accept(v, msg.ack),
            Section::Settings(v) => settings.accept(v, msg.ack),
            Section::Groups(v) => groups.accept(v, msg.ack),
        };
        accept(first);
        // Drain anything already queued, keeping only the newest per section.
        while let Ok(msg) = rx.try_recv() {
            accept(msg);
        }

        if let Some(v) = forwards.payload.take() {
            let result = store.save_forwards(v).await;
            if let Err(e) = &result {
                tracing::error!(error = %e, "failed to persist forwards");
            }
            forwards.resolve(&result);
        }
        if let Some(v) = settings.payload.take() {
            let result = store.save_settings(v).await;
            if let Err(e) = &result {
                tracing::error!(error = %e, "failed to persist settings");
            }
            settings.resolve(&result);
        }
        if let Some(v) = groups.payload.take() {
            let result = store.save_groups(v).await;
            if let Err(e) = &result {
                tracing::error!(error = %e, "failed to persist groups");
            }
            groups.resolve(&result);
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
    /// In-memory log ring buffer (cap 500, newest-first; not persisted). Shared
    /// with the tracing layer (spec 03 §18) so the layer's writes and the
    /// `get_logs` command's reads hit the same buffer.
    logs: Arc<LogBuffer>,
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
        logs: Arc<LogBuffer>,
        doc: ConfigDocument,
    ) -> Self {
        let keychain_available = credentials.keychain_available();
        // Attach the app handle so log appends now emit `log://line` to the FE.
        logs.set_app_handle(app.clone());
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
            logs,
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
            logs: Arc::new(LogBuffer::new()),
            credentials,
            keychain_available,
            config_store: None,
            persist_tx: None,
            app: RwLock::new(None),
        }
    }

    /// The `AppHandle` for emitting events / firing notifications; `None` in
    /// headless engine tests (event emission + notifications are no-ops).
    pub fn app_handle(&self) -> Option<tauri::AppHandle> {
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

    /// Insert or replace a config in the RAM mirror, preserving display position
    /// on replace. RAM-only: command handlers pair this with
    /// [`persist_forwards`](Self::persist_forwards) to flush + surface errors.
    pub fn upsert_config(&self, config: ForwardConfig) {
        if let Ok(mut g) = self.configs.write() {
            match g.iter_mut().find(|c| c.id == config.id) {
                Some(existing) => *existing = config,
                None => g.push(config),
            }
        }
    }

    /// Remove a config by id from the RAM mirror. Returns whether it existed.
    /// RAM-only (see [`upsert_config`](Self::upsert_config)).
    pub fn remove_config(&self, id: &str) -> bool {
        if let Ok(mut g) = self.configs.write() {
            let before = g.len();
            g.retain(|c| c.id != id);
            g.len() != before
        } else {
            false
        }
    }

    /// Replace the entire forwards list (reorder / backup replace). RAM-only.
    pub fn replace_configs(&self, configs: Vec<ForwardConfig>) {
        if let Ok(mut g) = self.configs.write() {
            *g = configs;
        }
    }

    /// Snapshot of all configs in display order.
    pub fn configs_snapshot(&self) -> Vec<ForwardConfig> {
        self.configs.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Snapshot of all groups.
    pub fn groups_snapshot(&self) -> Vec<TunnelGroup> {
        self.groups.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Insert or replace a group in the RAM mirror. RAM-only (pair with
    /// [`persist_groups`](Self::persist_groups)).
    pub fn upsert_group(&self, group: TunnelGroup) {
        if let Ok(mut g) = self.groups.write() {
            match g.iter_mut().find(|x| x.id == group.id) {
                Some(existing) => *existing = group,
                None => g.push(group),
            }
        }
    }

    /// Remove a group by id from the RAM mirror. Returns whether it existed.
    pub fn remove_group(&self, id: &str) -> bool {
        if let Ok(mut g) = self.groups.write() {
            let before = g.len();
            g.retain(|x| x.id != id);
            g.len() != before
        } else {
            false
        }
    }

    /// Replace the entire groups list (backup replace). RAM-only.
    pub fn replace_groups(&self, groups: Vec<TunnelGroup>) {
        if let Ok(mut g) = self.groups.write() {
            *g = groups;
        }
    }

    /// Current settings.
    pub fn settings_snapshot(&self) -> AppSettings {
        self.settings.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Replace settings in the RAM mirror. RAM-only (pair with
    /// [`persist_settings`](Self::persist_settings)).
    pub fn set_settings(&self, settings: AppSettings) {
        if let Ok(mut g) = self.settings.write() {
            *g = settings;
        }
    }

    /// The shared log ring buffer (spec 03 §18).
    pub fn log_buffer(&self) -> &LogBuffer {
        &self.logs
    }

    /// Whether the OS keychain is usable (false → plaintext fallback in use →
    /// the UI shows a persistent warning).
    pub fn keychain_available(&self) -> bool {
        self.keychain_available
    }

    /// Build the full boot/rehydrate snapshot (spec 04 §8). `update` is supplied
    /// by the caller (the updater is wired in M6; M4 passes a not-available
    /// default). Runtimes are read live from the registry.
    pub fn app_snapshot(&self, update: UpdateStatus) -> AppSnapshot {
        AppSnapshot {
            forwards: self.configs_snapshot(),
            groups: self.groups_snapshot(),
            settings: self.settings_snapshot(),
            logs: self.logs.snapshot(),
            runtimes: self.registry.all_runtimes(),
            update,
            keychain_available: self.keychain_available,
        }
    }

    // --- event emit helpers (no-op when headless) ---

    /// Emit `forwards://changed` with the current full list (CRUD/reorder).
    pub fn emit_forwards_changed(&self) {
        if let Some(app) = self.app_handle() {
            let _ = app.emit(events::FORWARDS_CHANGED, self.configs_snapshot());
        }
    }

    /// Emit `groups://changed` with the current full group list.
    pub fn emit_groups_changed(&self) {
        if let Some(app) = self.app_handle() {
            let _ = app.emit(events::GROUPS_CHANGED, self.groups_snapshot());
        }
    }

    /// Emit `settings://changed` with the current settings.
    pub fn emit_settings_changed(&self) {
        if let Some(app) = self.app_handle() {
            let _ = app.emit(events::SETTINGS_CHANGED, self.settings_snapshot());
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

    /// Store a password and SURFACE any failure to the caller (used by the
    /// `set_forward_password` command). The `keyring` set does blocking OS calls,
    /// so it runs on `spawn_blocking` to avoid stalling a tokio worker (F38). The
    /// secret is moved into the task and never logged.
    pub async fn set_password_checked(&self, id: &str, pw: String) -> Result<(), AppError> {
        let credentials = self.credentials.clone();
        let account = id.to_string();
        tokio::task::spawn_blocking(move || credentials.set_password(&account, &pw))
            .await
            .map_err(|e| AppError::Credential(format!("password store task failed: {e}")))?
    }

    /// Delete a password from every store and SURFACE any failure (used by the
    /// `clear_forward_password` command). Runs on `spawn_blocking` (F38).
    pub async fn delete_password_checked(&self, id: &str) -> Result<(), AppError> {
        let credentials = self.credentials.clone();
        let account = id.to_string();
        tokio::task::spawn_blocking(move || credentials.delete_password(&account))
            .await
            .map_err(|e| AppError::Credential(format!("password delete task failed: {e}")))?
    }

    // --- persistence helpers (enqueue to the single ordered writer; F37) ---

    /// Enqueue `section` for the ordered writer and AWAIT the actual write
    /// result, so a command can surface a persist failure to the user (F37 M4
    /// follow-up). The single-writer ordering guarantee is preserved — this only
    /// adds a completion signal. Headless (no `persist_tx`) is a no-op `Ok`
    /// (engine tests do not persist).
    async fn persist_section(&self, section: Section) -> Result<(), AppError> {
        let Some(tx) = &self.persist_tx else {
            return Ok(());
        };
        let (ack_tx, ack_rx) = oneshot::channel();
        tx.send(PersistMsg {
            section,
            ack: Some(ack_tx),
        })
        .map_err(|_| AppError::Storage("persistence writer is not running".into()))?;
        ack_rx
            .await
            .map_err(|_| AppError::Storage("persistence writer dropped before ack".into()))?
    }

    /// Persist the current forwards list and await the outcome (F37).
    pub async fn persist_forwards(&self) -> Result<(), AppError> {
        self.persist_section(Section::Forwards(self.configs_snapshot()))
            .await
    }

    /// Persist the current settings and await the outcome (F37).
    pub async fn persist_settings(&self) -> Result<(), AppError> {
        self.persist_section(Section::Settings(self.settings_snapshot()))
            .await
    }

    /// Persist the current groups list and await the outcome (F37).
    pub async fn persist_groups(&self) -> Result<(), AppError> {
        self.persist_section(Section::Groups(self.groups_snapshot()))
            .await
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
            tx.send(PersistMsg {
                section: Section::Forwards(snapshot),
                ack: None,
            })
            .expect("enqueue");
        }
        // The final, newest snapshot.
        let last = vec![forward_with_port("f", 9999), forward_with_port("g", 8888)];
        tx.send(PersistMsg {
            section: Section::Forwards(last.clone()),
            ack: None,
        })
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
            tx.send(PersistMsg {
                section: Section::Forwards(vec![forward_with_port("f", 2000 + i)]),
                ack: None,
            })
            .expect("enqueue fwd");
            let settings = AppSettings {
                auto_reconnect_max_retries: i as u32,
                ..AppSettings::default()
            };
            tx.send(PersistMsg {
                section: Section::Settings(settings),
                ack: None,
            })
            .expect("enqueue settings");
        }
        let last_forwards = vec![forward_with_port("final", 7777)];
        let last_settings = AppSettings {
            auto_reconnect_max_retries: 99,
            show_in_dock: true,
            ..AppSettings::default()
        };
        tx.send(PersistMsg {
            section: Section::Forwards(last_forwards.clone()),
            ack: None,
        })
        .expect("enqueue last fwd");
        tx.send(PersistMsg {
            section: Section::Settings(last_settings.clone()),
            ack: None,
        })
        .expect("enqueue last settings");

        drop(tx);
        persist_writer_loop(store.clone(), rx).await;

        let doc = store.load().await.expect("load");
        assert_eq!(doc.forwards, last_forwards);
        assert_eq!(doc.settings, last_settings);
    }

    /// F37 M4 follow-up: a mutation's ack receives the ACTUAL write result, so a
    /// command can surface a persist success/failure to the user. Here the write
    /// succeeds and both coalesced acks (for the same section) resolve `Ok`.
    #[tokio::test]
    async fn writer_acks_report_write_outcome() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = Arc::new(ConfigStore::from_config_dir(dir.path()));
        let (tx, rx) = mpsc::unbounded_channel::<PersistMsg>();

        let (ack1_tx, ack1_rx) = oneshot::channel();
        let (ack2_tx, ack2_rx) = oneshot::channel();
        tx.send(PersistMsg {
            section: Section::Forwards(vec![forward_with_port("a", 1111)]),
            ack: Some(ack1_tx),
        })
        .expect("enqueue 1");
        tx.send(PersistMsg {
            section: Section::Forwards(vec![forward_with_port("a", 2222)]),
            ack: Some(ack2_tx),
        })
        .expect("enqueue 2");
        drop(tx);
        persist_writer_loop(store.clone(), rx).await;

        // Both coalesced mutations get the (Ok) result of the single write.
        assert!(ack1_rx.await.expect("ack1 delivered").is_ok());
        assert!(ack2_rx.await.expect("ack2 delivered").is_ok());
        // The newest snapshot is what actually landed on disk.
        let doc = store.load().await.expect("load");
        assert_eq!(doc.forwards, vec![forward_with_port("a", 2222)]);
    }
}
