//! v1→v2 migration (spec 04 §12).
//!
//! v1 (Flutter) wrote `tunnel_pilot_config.json` under a per-OS app-support dir
//! whose folder name Tauri's `app_config_dir` does **NOT** reproduce (macOS
//! bundle id `com.kalfian.tunnelpilot`, no underscore; Windows two-level
//! `%APPDATA%\kalfian\Tunnel Pilot\`). Relying on Tauri dir resolution alone
//! would silently lose every user's config on upgrade (F2), so we probe the
//! **hardcoded** v1 path for the current OS. Linux never shipped v1 (F17) → no
//! probe, fresh install only.
//!
//! On first v2 boot, if the v2 location has no config (or a config whose
//! `schemaVersion < 2`, i.e. a same-dir v1 file), we import the v1 `forwards` +
//! `settings`, move each plaintext `sshPassword` into the credential store
//! (setting `hasStoredPassword`, never carrying the secret into the v2 file),
//! write a `.v1-backup` copy of the source, and lay down the v2 document
//! atomically. Idempotent: once `schemaVersion == 2` the migration is skipped.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::credentials::CredentialStore;
use crate::error::AppError;
use crate::state::models::{AppSettings, ForwardConfig};
use crate::storage::config_file::{ConfigDocument, ConfigStore, SCHEMA_VERSION};

/// Outcome of a migration attempt (for logging / the boot summary).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrationOutcome {
    /// A v1 config was found and imported.
    pub migrated: bool,
    /// Number of forwards imported.
    pub imported_forwards: usize,
    /// Number of plaintext passwords moved into the credential store.
    pub migrated_passwords: usize,
    /// The v1 source path that was imported, if any.
    pub source: Option<PathBuf>,
}

/// The three v1 release targets — drives the hardcoded per-OS path probe. Kept
/// host-independent so [`v1_config_path_for`] can be unit-tested for every OS
/// from any host; the live [`v1_config_path`] selects the current one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V1Os {
    MacOs,
    Windows,
    Linux,
}

/// Build the hardcoded v1 config path for a given OS relative to its base dir
/// (macOS: `$HOME`; Windows: `%APPDATA%`). Linux returns `None` — v1 never
/// shipped a Linux release (F17), so there is nothing to probe. Pure function
/// (no env / filesystem) so all three OS branches are testable from any host.
pub(crate) fn v1_config_path_for(os: V1Os, base: &Path) -> Option<PathBuf> {
    match os {
        V1Os::MacOs => Some(
            base.join("Library")
                .join("Application Support")
                .join("com.kalfian.tunnelpilot")
                .join(crate::storage::config_file::CONFIG_FILE_NAME),
        ),
        V1Os::Windows => Some(
            base.join("kalfian")
                .join("Tunnel Pilot")
                .join(crate::storage::config_file::CONFIG_FILE_NAME),
        ),
        V1Os::Linux => None,
    }
}

/// The hardcoded v1 config path for the CURRENT OS, resolved against the real
/// base dir. `None` on Linux (no probe) or when the base env var is missing.
fn v1_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        v1_config_path_for(V1Os::MacOs, Path::new(&home))
    }
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        v1_config_path_for(V1Os::Windows, Path::new(&appdata))
    }
    #[cfg(target_os = "linux")]
    {
        None
    }
}

/// Lenient v1 forward: reuses [`ForwardConfig`] for the shared (non-secret)
/// fields (its `#[serde(default)]`s tolerate missing v2-only keys) and captures
/// the v1-only plaintext `sshPassword` separately so it can be moved into the
/// credential store.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1Forward {
    #[serde(flatten)]
    config: ForwardConfig,
    #[serde(default)]
    ssh_password: Option<String>,
}

/// Lenient v1 document. `settings` is captured as a raw value and converted
/// with a default fallback so a partial/absent v1 settings block never fails
/// the whole migration.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1Document {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    forwards: Vec<V1Forward>,
    #[serde(default)]
    settings: Option<serde_json::Value>,
}

/// Boot-time migration entry point.
///
/// Decides whether migration is needed and runs it. No-op when the v2 config
/// already has `schemaVersion >= 2`. Never crashes on a missing/absent v1 path
/// (fresh install, esp. Linux).
pub async fn migrate_if_needed(
    store: &ConfigStore,
    creds: &CredentialStore,
) -> Result<MigrationOutcome, AppError> {
    // 1. Inspect the v2 location.
    if store.exists() {
        match tokio::fs::read(store.path()).await {
            Ok(bytes) if !bytes.is_empty() => {
                let schema = serde_json::from_slice::<ConfigDocument>(&bytes)
                    .map(|d| d.schema_version)
                    .unwrap_or(0);
                if schema >= SCHEMA_VERSION {
                    // Already v2 — nothing to do (idempotent).
                    return Ok(MigrationOutcome::default());
                }
                // A config exists at the v2 path but is pre-v2 (missing/`<2`
                // schemaVersion) → treat it as a same-dir v1 file (spec 04 §12).
                let src = store.path().to_path_buf();
                return import(&src, store, creds).await;
            }
            // Empty/unreadable file → fall through to the v1 probe.
            _ => {}
        }
    }

    // 2. v2 location has no usable config → probe the hardcoded v1 path.
    let Some(v1_path) = v1_config_path() else {
        // Linux (no probe) or missing base env → fresh install.
        return Ok(MigrationOutcome::default());
    };
    if !v1_path.exists() {
        return Ok(MigrationOutcome::default());
    }
    import(&v1_path, store, creds).await
}

/// Import a v1 config file at `src` into the v2 `store`, moving plaintext
/// passwords into `creds`. Writes a `.v1-backup` copy of the source, then lays
/// down the v2 document atomically. Exposed for tests (drive it with an
/// explicit source + in-memory/fallback credential store).
pub async fn import(
    src: &Path,
    store: &ConfigStore,
    creds: &CredentialStore,
) -> Result<MigrationOutcome, AppError> {
    let bytes = tokio::fs::read(src).await?;
    let v1: V1Document = serde_json::from_slice(&bytes)
        .map_err(|e| AppError::Config(format!("v1 config parse failed: {e}")))?;

    // Preserve the original v1 file next to the source (idempotent — skip if a
    // backup already exists so a re-run never clobbers the first snapshot).
    let backup = append_suffix(src, ".v1-backup");
    if !backup.exists() {
        tokio::fs::write(&backup, &bytes).await?;
    }

    let mut forwards = Vec::with_capacity(v1.forwards.len());
    let mut migrated_passwords = 0usize;
    for entry in v1.forwards {
        let mut config = entry.config;
        match entry.ssh_password {
            Some(pw) if !pw.is_empty() => {
                // Move the plaintext secret into the keychain/fallback store;
                // never carry it into the v2 config (AGENTS §8).
                creds.set_password(&config.id, &pw)?;
                config.has_stored_password = true;
                migrated_passwords += 1;
            }
            _ => {
                // No usable password — identity-file or password-on-connect.
                config.has_stored_password = false;
            }
        }
        forwards.push(config);
    }

    let settings = v1
        .settings
        .and_then(|v| serde_json::from_value::<AppSettings>(v).ok())
        .unwrap_or_default();

    let imported_forwards = forwards.len();
    let doc = ConfigDocument {
        schema_version: SCHEMA_VERSION,
        forwards,
        groups: Vec::new(),
        settings,
    };
    store.write_document(&doc).await?;

    tracing::info!(
        source = %src.display(),
        forwards = imported_forwards,
        passwords = migrated_passwords,
        v1_schema = v1.schema_version,
        "migrated v1 config to v2"
    );

    Ok(MigrationOutcome {
        migrated: true,
        imported_forwards,
        migrated_passwords,
        source: Some(src.to_path_buf()),
    })
}

/// Append a literal suffix to a full path (e.g. `foo.json` → `foo.json.v1-backup`).
/// Unlike `Path::with_extension`, this keeps the existing extension.
fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::config_file::CONFIG_FILE_NAME;

    /// A real-shape v1 `tunnel_pilot_config.json` as written by the Flutter
    /// `StorageService` (`toJson` includes plaintext `sshPassword`; no
    /// `schemaVersion`; no `groups`). Two forwards: one password, one identity.
    fn v1_config_json() -> &'static str {
        r#"{
  "forwards": [
    {
      "id": "3f2a-uuid-pw",
      "name": "Prod DB",
      "sshHost": "bastion.example.com",
      "sshPort": 2222,
      "sshUsername": "deploy",
      "sshPassword": "hunter2",
      "identityFilePath": null,
      "localBindAddress": "127.0.0.1",
      "localPort": 5432,
      "remoteHost": "db.internal",
      "remotePort": 5432,
      "keepAliveIntervalSec": 45,
      "keepAliveMaxCount": 4
    },
    {
      "id": "a1b2-uuid-key",
      "name": "Cache",
      "sshHost": "jump.example.com",
      "sshPort": 22,
      "sshUsername": "ops",
      "sshPassword": null,
      "identityFilePath": "/Users/me/.ssh/id_ed25519",
      "localBindAddress": "0.0.0.0",
      "localPort": 6379,
      "remoteHost": "cache.internal",
      "remotePort": 6379,
      "keepAliveIntervalSec": 30,
      "keepAliveMaxCount": 5
    }
  ],
  "settings": {
    "launchAtLogin": false,
    "showNotifications": true,
    "themeMode": "dark",
    "autoReconnect": true,
    "autoReconnectDelaySec": 7,
    "autoReconnectMaxRetries": 9,
    "showInDock": true,
    "autoCheckUpdates": false,
    "lastSkippedVersion": "1.4.1"
  }
}"#
    }

    fn assert_v1_imported(doc: &ConfigDocument) {
        assert_eq!(doc.schema_version, SCHEMA_VERSION);
        assert!(doc.groups.is_empty(), "v2 groups default to empty");
        assert_eq!(doc.forwards.len(), 2);

        // Forward data survives without loss (names, hosts, ports, keepalive).
        let pw = &doc.forwards[0];
        assert_eq!(pw.id, "3f2a-uuid-pw");
        assert_eq!(pw.name, "Prod DB");
        assert_eq!(pw.ssh_port, 2222);
        assert_eq!(pw.keep_alive_interval_sec, 45);
        assert_eq!(pw.keep_alive_max_count, 4);
        assert!(pw.has_stored_password, "password forward flagged");
        assert!(
            pw.group_id.is_none() && pw.tags.is_empty(),
            "v2 defaults applied"
        );

        let key = &doc.forwards[1];
        assert_eq!(
            key.identity_file_path.as_deref(),
            Some("/Users/me/.ssh/id_ed25519")
        );
        assert_eq!(key.local_bind_address, "0.0.0.0");
        assert!(
            !key.has_stored_password,
            "identity forward has no stored password"
        );

        // Settings carried across, including lastSkippedVersion.
        assert!(!doc.settings.launch_at_login);
        assert_eq!(doc.settings.auto_reconnect_delay_sec, 7);
        assert_eq!(doc.settings.auto_reconnect_max_retries, 9);
        assert!(doc.settings.show_in_dock);
        assert_eq!(doc.settings.last_skipped_version.as_deref(), Some("1.4.1"));
    }

    // ---- Hardcoded per-OS path probe (F2) — testable from any host ----

    #[test]
    fn v1_path_macos_uses_no_underscore_bundle_id() {
        let base = Path::new("/Users/me");
        let p = v1_config_path_for(V1Os::MacOs, base).expect("macos path");
        assert_eq!(
            p,
            Path::new(
                "/Users/me/Library/Application Support/com.kalfian.tunnelpilot/tunnel_pilot_config.json"
            )
        );
        // Guard against the WITH-underscore Dart package name creeping in.
        assert!(!p.to_string_lossy().contains("tunnel_pilot/"));
    }

    #[test]
    fn v1_path_windows_uses_two_level_appdata() {
        let base = Path::new(r"C:\Users\me\AppData\Roaming");
        let p = v1_config_path_for(V1Os::Windows, base).expect("windows path");
        assert!(p.ends_with(
            Path::new("kalfian")
                .join("Tunnel Pilot")
                .join(CONFIG_FILE_NAME)
        ));
    }

    #[test]
    fn v1_path_linux_never_probes() {
        // F17: v1 never shipped on Linux → no probe path at all.
        assert_eq!(v1_config_path_for(V1Os::Linux, Path::new("/home/me")), None);
    }

    #[test]
    fn probe_finds_fixture_at_hardcoded_path() {
        // Assert the hardcoded path builder resolves to a file we actually
        // place there (simulating the real v1 install), for the current host OS.
        let dir = tempfile::tempdir().expect("temp home");
        #[cfg(target_os = "macos")]
        let (os, base) = (V1Os::MacOs, dir.path());
        #[cfg(target_os = "windows")]
        let (os, base) = (V1Os::Windows, dir.path());
        #[cfg(target_os = "linux")]
        let (os, base) = (V1Os::Linux, dir.path());

        match v1_config_path_for(os, base) {
            Some(p) => {
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, v1_config_json()).unwrap();
                assert!(p.exists(), "hardcoded probe path locates the fixture");
            }
            None => {
                // Linux: no probe — assert there is genuinely no path.
                assert!(matches!(os, V1Os::Linux));
            }
        }
    }

    // ---- End-to-end import: keychain route + fallback route ----

    #[tokio::test]
    async fn import_moves_password_into_keychain() {
        let dir = tempfile::tempdir().expect("temp dir");
        let src = dir.path().join(CONFIG_FILE_NAME);
        tokio::fs::write(&src, v1_config_json()).await.unwrap();

        let v2 = dir.path().join("v2");
        let store = ConfigStore::from_config_dir(&v2);
        let creds = CredentialStore::in_memory();

        let outcome = import(&src, &store, &creds).await.expect("import");
        assert!(outcome.migrated);
        assert_eq!(outcome.imported_forwards, 2);
        assert_eq!(outcome.migrated_passwords, 1);

        let doc = store.load().await.expect("reload v2");
        assert_v1_imported(&doc);

        // Password landed in the (in-memory) keychain, keyed by forward id.
        assert_eq!(
            creds.get_password("3f2a-uuid-pw").expect("get"),
            Some("hunter2".to_string())
        );
        // ...and NOT in the v2 config file (AGENTS §8).
        let raw = tokio::fs::read_to_string(store.path()).await.unwrap();
        assert!(!raw.contains("hunter2"));
        assert!(!raw.contains("sshPassword"));

        // `.v1-backup` written next to the source.
        assert!(append_suffix(&src, ".v1-backup").exists());
    }

    #[tokio::test]
    async fn import_falls_back_to_secrets_file_when_keychain_unavailable() {
        let dir = tempfile::tempdir().expect("temp dir");
        let src = dir.path().join(CONFIG_FILE_NAME);
        tokio::fs::write(&src, v1_config_json()).await.unwrap();

        let store = ConfigStore::from_config_dir(&dir.path().join("v2"));
        let secrets = dir.path().join("tunnel_pilot_secrets.json");
        let creds = CredentialStore::fallback_only(secrets.clone());

        let outcome = import(&src, &store, &creds).await.expect("import");
        assert_eq!(outcome.migrated_passwords, 1);
        assert!(
            !creds.keychain_available(),
            "warning flag drives the UI banner"
        );

        // Secret is retrievable, lives in the fallback file, never in v2 config.
        assert_eq!(
            creds.get_password("3f2a-uuid-pw").expect("get"),
            Some("hunter2".to_string())
        );
        assert!(secrets.exists());
        let cfg_raw = tokio::fs::read_to_string(store.path()).await.unwrap();
        assert!(!cfg_raw.contains("hunter2"));
    }

    // ---- migrate_if_needed: idempotency + fresh install ----

    #[tokio::test]
    async fn migrate_if_needed_skips_existing_v2_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ConfigStore::from_config_dir(dir.path());
        // Seed an already-v2 document.
        store
            .write_document(&ConfigDocument::default())
            .await
            .unwrap();

        let creds = CredentialStore::in_memory();
        let outcome = migrate_if_needed(&store, &creds).await.expect("migrate");
        assert_eq!(
            outcome,
            MigrationOutcome::default(),
            "no-op when already v2"
        );
    }

    #[tokio::test]
    async fn migrate_in_place_upgrades_pre_v2_config_at_v2_path() {
        // A v1 file sitting AT the v2 path (missing schemaVersion) is upgraded
        // in place and becomes idempotent afterwards.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ConfigStore::from_config_dir(dir.path());
        tokio::fs::write(store.path(), v1_config_json())
            .await
            .unwrap();

        let creds = CredentialStore::in_memory();
        let first = migrate_if_needed(&store, &creds).await.expect("migrate");
        assert!(first.migrated);
        assert_eq!(first.imported_forwards, 2);

        let doc = store.load().await.expect("reload");
        assert_v1_imported(&doc);

        // Re-run is a no-op (schemaVersion == 2 now).
        let second = migrate_if_needed(&store, &creds).await.expect("re-migrate");
        assert_eq!(second, MigrationOutcome::default());
    }

    #[tokio::test]
    async fn migrate_if_needed_fresh_install_is_noop() {
        // No v2 file and (on the test host) no v1 file at the hardcoded path →
        // fresh install, no crash, no migration.
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ConfigStore::from_config_dir(dir.path());
        let creds = CredentialStore::in_memory();
        let outcome = migrate_if_needed(&store, &creds).await.expect("migrate");
        // migrated is false; on a dev machine with no real v1 install this is a
        // clean no-op. (If a real v1 config exists on this host it would import,
        // but the v2 path is a throwaway temp dir so it never clobbers anything.)
        if outcome.migrated {
            // A real v1 config happened to exist; assert it at least parsed.
            assert!(outcome.source.is_some());
        } else {
            assert_eq!(outcome, MigrationOutcome::default());
        }
    }

    // ---- Lenient v1-BACKUP import (F19): version 1, no groups, sshPassword ----
    // (Backup parsing lives in storage::backup; here we assert the shared
    //  ForwardConfig deserialization tolerates a v1 backup entry.)

    #[test]
    fn v1_backup_forward_entry_parses_without_v2_fields() {
        // A v1 `toJsonForBackup` entry: no hasStoredPassword/groupId/tags, and a
        // stray legacy sshPassword that must be ignorable.
        let entry = r#"{
            "id": "b-1",
            "name": "Legacy",
            "sshHost": "h",
            "sshUsername": "u",
            "sshPassword": "should-be-ignored",
            "identityFilePath": null,
            "localPort": 1000,
            "remoteHost": "r",
            "remotePort": 2000,
            "keepAliveIntervalSec": 30,
            "keepAliveMaxCount": 5
        }"#;
        let cfg: ForwardConfig = serde_json::from_str(entry).expect("lenient parse");
        assert_eq!(cfg.ssh_port, 22, "default applied");
        assert_eq!(cfg.local_bind_address, "127.0.0.1", "default applied");
        assert!(!cfg.has_stored_password);
        assert!(cfg.group_id.is_none());
        assert!(cfg.tags.is_empty());
        // The legacy sshPassword key is simply not a field on ForwardConfig, so
        // serde ignores it — no secret is imported.
    }
}
