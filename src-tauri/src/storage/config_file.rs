//! Atomic read-merge-write of `tunnel_pilot_config.json`; corruption →
//! `.corrupted-<ts>` quarantine; single canonical `app_config_dir` (F2, spec 03
//! §7, 04 §9).
//!
//! The full document (`schemaVersion` + `forwards` + `groups` + `settings`) is
//! the on-disk shape. `forwards`, `groups`, and `settings` are saved
//! **independently** via read-merge-write of the whole file (mirrors v1's
//! `saveForwards`/`saveSettings`): a save loads the current document, replaces
//! exactly one section, and writes the whole thing back — so mutating settings
//! never drops forwards and vice versa. Every write is atomic (tmp + fsync +
//! rename) and serialized behind an async `Mutex` so concurrent saves cannot
//! interleave. Passwords never appear in this file (spec 04 §10; AGENTS §8).

use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::state::models::{AppSettings, ForwardConfig, TunnelGroup};

/// Canonical config filename in the v2 `app_config_dir` (spec 03 §7). Same name
/// as v1 on purpose; the directory differs (handled by migration, spec 04 §12).
pub const CONFIG_FILE_NAME: &str = "tunnel_pilot_config.json";

/// Current on-disk schema version (spec 04 §13). Gates v1→v2 migration.
pub const SCHEMA_VERSION: u32 = 2;

/// The full on-disk config document (spec 04 §9). `#[serde(default)]` on every
/// section keeps loads lenient: a file missing a section (e.g. a v1 file with
/// no `groups`/`schemaVersion`) still parses, defaulting the absent parts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDocument {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub forwards: Vec<ForwardConfig>,
    #[serde(default)]
    pub groups: Vec<TunnelGroup>,
    #[serde(default)]
    pub settings: AppSettings,
}

impl Default for ConfigDocument {
    /// A fresh v2 document (empty lists, default settings, current schema).
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            forwards: Vec::new(),
            groups: Vec::new(),
            settings: AppSettings::default(),
        }
    }
}

/// Persisted config store over a single JSON file. Cheap to construct; all
/// state lives on disk. Writes serialize behind `write_lock` (spec 03 §7).
pub struct ConfigStore {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl ConfigStore {
    /// Point the store at an explicit config file path.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            write_lock: Mutex::new(()),
        }
    }

    /// Build from the canonical v2 config directory (`app_config_dir`),
    /// appending [`CONFIG_FILE_NAME`].
    pub fn from_config_dir(config_dir: &Path) -> Self {
        Self::new(config_dir.join(CONFIG_FILE_NAME))
    }

    /// Path of the backing config file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the config file physically exists (migration uses this to decide
    /// whether the v2 location is populated).
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Load the full document. Missing file → a fresh v2 default. Corrupt JSON →
    /// the bad file is copied to `<name>.corrupted-<timestamp>`, an error is
    /// logged, and a fresh default is returned (never crash on bad config —
    /// spec 03 §7).
    pub async fn load(&self) -> Result<ConfigDocument, AppError> {
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConfigDocument::default())
            }
            Err(e) => return Err(AppError::from(e)),
        };

        if bytes.is_empty() {
            return Ok(ConfigDocument::default());
        }

        match serde_json::from_slice::<ConfigDocument>(&bytes) {
            Ok(doc) => Ok(doc),
            Err(e) => {
                self.quarantine_corrupt(&bytes, &e).await;
                Ok(ConfigDocument::default())
            }
        }
    }

    /// Read-merge-write: replace only `forwards`, preserving `groups`/`settings`.
    pub async fn save_forwards(&self, forwards: Vec<ForwardConfig>) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        let mut doc = self.read_merge_base().await;
        doc.forwards = forwards;
        doc.schema_version = SCHEMA_VERSION;
        self.write_document_locked(&doc).await
    }

    /// Read-merge-write: replace only `settings`, preserving `forwards`/`groups`.
    pub async fn save_settings(&self, settings: AppSettings) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        let mut doc = self.read_merge_base().await;
        doc.settings = settings;
        doc.schema_version = SCHEMA_VERSION;
        self.write_document_locked(&doc).await
    }

    /// Read-merge-write: replace only `groups`, preserving `forwards`/`settings`.
    pub async fn save_groups(&self, groups: Vec<TunnelGroup>) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        let mut doc = self.read_merge_base().await;
        doc.groups = groups;
        doc.schema_version = SCHEMA_VERSION;
        self.write_document_locked(&doc).await
    }

    /// Write a whole document atomically (used by migration to lay down the
    /// freshly imported v2 file). Serialized behind the write lock.
    pub async fn write_document(&self, doc: &ConfigDocument) -> Result<(), AppError> {
        let _guard = self.write_lock.lock().await;
        self.write_document_locked(doc).await
    }

    /// The merge base for a read-merge-write. On any read/parse failure returns
    /// a fresh default (the public [`load`](Self::load) at boot already
    /// quarantined a corrupt file, so we do not quarantine again here — that
    /// would race and double-write sidecars). Mirrors v1's save path, which
    /// silently discards unreadable content before overwriting.
    async fn read_merge_base(&self) -> ConfigDocument {
        match tokio::fs::read(&self.path).await {
            Ok(bytes) if !bytes.is_empty() => {
                serde_json::from_slice::<ConfigDocument>(&bytes).unwrap_or_default()
            }
            _ => ConfigDocument::default(),
        }
    }

    /// Atomic write: serialize → write `<path>.tmp` → fsync → rename over the
    /// target. Rename is atomic within a directory on all three OSes, so a
    /// process killed mid-write can never leave a truncated config. Caller MUST
    /// hold `write_lock`.
    async fn write_document_locked(&self, doc: &ConfigDocument) -> Result<(), AppError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent).await?;
            }
        }

        let bytes = serde_json::to_vec_pretty(doc)?;
        let tmp = self.tmp_path();

        let mut file = tokio::fs::File::create(&tmp).await?;
        file.write_all(&bytes).await?;
        file.sync_all().await?;
        drop(file);

        if let Err(e) = tokio::fs::rename(&tmp, &self.path).await {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(AppError::from(e));
        }
        Ok(())
    }

    fn tmp_path(&self) -> PathBuf {
        let mut name = self.path.as_os_str().to_owned();
        name.push(".tmp");
        PathBuf::from(name)
    }

    /// Copy a corrupt config to a timestamped `.corrupted-*` sidecar and log an
    /// error. Best-effort — a failure to quarantine must not itself crash the
    /// load (we still start with defaults).
    async fn quarantine_corrupt(&self, bytes: &[u8], err: &serde_json::Error) {
        let mut name = self.path.as_os_str().to_owned();
        name.push(format!(
            ".corrupted-{}",
            Utc::now().format("%Y%m%dT%H%M%S%3fZ")
        ));
        let sidecar = PathBuf::from(name);
        match tokio::fs::write(&sidecar, bytes).await {
            Ok(()) => tracing::error!(
                path = %self.path.display(),
                sidecar = %sidecar.display(),
                error = %err,
                "config file is corrupt; quarantined and starting with defaults"
            ),
            Err(io) => tracing::error!(
                path = %self.path.display(),
                error = %err,
                quarantine_error = %io,
                "config file is corrupt and could not be quarantined; starting with defaults"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::ThemeMode;

    fn store() -> (tempfile::TempDir, ConfigStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ConfigStore::from_config_dir(dir.path());
        (dir, store)
    }

    fn sample_forward(id: &str) -> ForwardConfig {
        ForwardConfig {
            id: id.to_string(),
            name: format!("fwd-{id}"),
            ssh_host: "bastion.example.com".into(),
            ssh_port: 22,
            ssh_username: "deploy".into(),
            identity_file_path: None,
            has_stored_password: true,
            local_bind_address: "127.0.0.1".into(),
            local_port: 5432,
            remote_host: "db.internal".into(),
            remote_port: 5432,
            keep_alive_interval_sec: 30,
            keep_alive_max_count: 5,
            group_id: None,
            tags: vec![],
        }
    }

    #[tokio::test]
    async fn load_missing_file_is_fresh_default() {
        let (_dir, store) = store();
        let doc = store.load().await.expect("load");
        assert_eq!(doc, ConfigDocument::default());
        assert_eq!(doc.schema_version, SCHEMA_VERSION);
    }

    #[tokio::test]
    async fn write_is_atomic_no_tmp_leftover() {
        let (_dir, store) = store();
        let mut doc = ConfigDocument::default();
        doc.forwards.push(sample_forward("a"));
        store.write_document(&doc).await.expect("write");

        // Target exists and round-trips; the tmp sibling was renamed away.
        assert!(store.path().exists());
        assert!(!store.tmp_path().exists(), "tmp file must not linger");
        let reloaded = store.load().await.expect("reload");
        assert_eq!(reloaded, doc);
    }

    #[tokio::test]
    async fn corrupt_json_quarantined_and_defaults_returned() {
        let (dir, store) = store();
        tokio::fs::write(store.path(), b"{ this is not valid json ")
            .await
            .expect("seed corrupt");

        let doc = store.load().await.expect("load never crashes");
        assert_eq!(doc, ConfigDocument::default());

        // Exactly one `.corrupted-*` sidecar was written with the original bytes.
        let mut sidecars = vec![];
        let mut rd = tokio::fs::read_dir(dir.path()).await.expect("read_dir");
        while let Some(entry) = rd.next_entry().await.expect("entry") {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.contains(".corrupted-") {
                sidecars.push(entry.path());
            }
        }
        assert_eq!(sidecars.len(), 1, "one corrupted sidecar expected");
        let quarantined = tokio::fs::read(&sidecars[0]).await.expect("read sidecar");
        assert_eq!(quarantined, b"{ this is not valid json ");
    }

    #[tokio::test]
    async fn save_settings_preserves_forwards_and_groups() {
        let (_dir, store) = store();
        let mut doc = ConfigDocument::default();
        doc.forwards.push(sample_forward("keep-me"));
        doc.groups.push(TunnelGroup {
            id: "grp".into(),
            name: "Prod".into(),
            color: Some("#EF4444".into()),
            order: 0,
            collapsed: false,
        });
        store.write_document(&doc).await.expect("seed");

        let settings = AppSettings {
            theme_mode: ThemeMode::Dark,
            auto_reconnect_max_retries: 9,
            ..AppSettings::default()
        };
        store
            .save_settings(settings.clone())
            .await
            .expect("save settings");

        let reloaded = store.load().await.expect("reload");
        assert_eq!(reloaded.settings, settings, "settings updated");
        assert_eq!(reloaded.forwards, doc.forwards, "forwards preserved");
        assert_eq!(reloaded.groups, doc.groups, "groups preserved");
    }

    #[tokio::test]
    async fn save_forwards_preserves_settings_sibling() {
        let (_dir, store) = store();
        let settings = AppSettings {
            show_in_dock: true,
            ..AppSettings::default()
        };
        store
            .save_settings(settings.clone())
            .await
            .expect("seed settings");

        store
            .save_forwards(vec![sample_forward("x"), sample_forward("y")])
            .await
            .expect("save forwards");

        let reloaded = store.load().await.expect("reload");
        assert_eq!(reloaded.settings, settings, "settings sibling untouched");
        assert_eq!(reloaded.forwards.len(), 2);
    }

    #[tokio::test]
    async fn secret_never_written_to_config_file() {
        // Sanity: the config document has no password field at all, so a save
        // can never leak one. Assert the serialized bytes contain no obvious
        // password key (AGENTS §8).
        let (_dir, store) = store();
        let mut doc = ConfigDocument::default();
        doc.forwards.push(sample_forward("a"));
        store.write_document(&doc).await.expect("write");
        let raw = tokio::fs::read_to_string(store.path()).await.expect("read");
        assert!(!raw.contains("sshPassword"));
        assert!(!raw.contains("\"password\""));
        assert!(raw.contains("\"hasStoredPassword\""));
    }
}
