//! Plaintext fallback secrets store — used ONLY when the OS keychain is
//! unavailable (e.g. headless Linux with no Secret Service / D-Bus).
//!
//! Kept in a **separate** file (`tunnel_pilot_secrets.json`) from the main
//! config so backup-strip and config-merge logic never has to know about
//! secrets (spec 04 §10). This file is `0600` where the OS supports it and is
//! **never** included in backups. When it is in use the UI surfaces a warning
//! driven by `keychain_available == false`.
//!
//! Security: the plaintext password lives ONLY inside `secrets[<forwardId>]`.
//! It is never logged, emitted over IPC, or serialized anywhere else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// On-disk shape of the fallback secrets file (spec 04 §10).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SecretsFile {
    schema_version: u32,
    /// `forwardId -> plaintext password`. `BTreeMap` gives deterministic
    /// on-disk ordering (stable diffs, easier tests).
    secrets: BTreeMap<String, String>,
}

impl Default for SecretsFile {
    fn default() -> Self {
        Self {
            schema_version: 1,
            secrets: BTreeMap::new(),
        }
    }
}

/// File-backed fallback secret store. Cheap to construct; all state lives on
/// disk so it stays correct across process restarts.
#[derive(Debug, Clone)]
pub struct FallbackStore {
    path: PathBuf,
}

impl FallbackStore {
    /// Point the store at its secrets file. The file is created lazily on the
    /// first write.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Path to the backing file (for the UI warning / diagnostics — never the
    /// contents).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a secrets file physically exists (used to decide if the warning
    /// is worth surfacing even when unread).
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    fn load(&self) -> Result<SecretsFile, AppError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                let file: SecretsFile = serde_json::from_slice(&bytes)?;
                Ok(file)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(SecretsFile::default()),
            Err(e) => Err(AppError::from(e)),
        }
    }

    /// Atomically persist `file`: write to a temp sibling, `0600` it, then
    /// rename over the target so a crash mid-write can never truncate the file.
    fn store(&self, file: &SecretsFile) -> Result<(), AppError> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        let bytes = serde_json::to_vec_pretty(file)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &bytes)?;
        restrict_permissions(&tmp)?;

        // Rename is atomic on the same filesystem; the temp sibling guarantees
        // that (parent dir + same fs).
        std::fs::rename(&tmp, &self.path).map_err(|e| {
            // Best-effort cleanup so we don't leak a `.tmp` on failure.
            let _ = std::fs::remove_file(&tmp);
            AppError::from(e)
        })?;
        restrict_permissions(&self.path)?;
        Ok(())
    }

    /// Store `secret` for `id`, replacing any existing value.
    pub fn set(&self, id: &str, secret: &str) -> Result<(), AppError> {
        let mut file = self.load()?;
        file.secrets.insert(id.to_string(), secret.to_string());
        self.store(&file)
    }

    /// Fetch the secret for `id`, if present.
    pub fn get(&self, id: &str) -> Result<Option<String>, AppError> {
        let file = self.load()?;
        Ok(file.secrets.get(id).cloned())
    }

    /// Remove the secret for `id`. Missing entry is a no-op (idempotent delete).
    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        let mut file = self.load()?;
        if file.secrets.remove(id).is_some() {
            self.store(&file)?;
        }
        Ok(())
    }
}

/// Tighten the secrets file to owner-only read/write (`0600`) on Unix. No-op on
/// platforms without POSIX permission bits (the fallback path is a Linux-
/// headless corner case anyway).
#[cfg(unix)]
fn restrict_permissions(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) -> Result<(), AppError> {
    Ok(())
}
