//! Backup format + lenient parse (spec 04 §11).
//!
//! A v2 export is `{ version, exportedAt, forwards, groups }` and strips
//! passwords by design (`hasStoredPassword` forced false; no secret ever
//! touches a backup — AGENTS §8). Import parsing is deliberately **lenient** so
//! a v1 backup still loads (F19): a v1 export is `{ version:1, exportedAt,
//! forwards:[...] }` with **no `groups`** key and possibly a stray legacy
//! `sshPassword` on each forward. `#[serde(default)]` on `groups` covers the
//! missing key; `ForwardConfig` simply has no `sshPassword` field so serde
//! drops it — no secret is ever imported.
//!
//! This module owns the wire format + validation only. Applying an import to
//! `AppState` (replace|merge, `ImportResult`) lands with the real command
//! surface in M4.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::models::{ForwardConfig, TunnelGroup};

/// Current backup format version (spec 04 §13). v1 backups are `version:1`.
pub const BACKUP_VERSION: u32 = 2;

/// On-disk backup document (spec 04 §11).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub version: u32,
    #[serde(default)]
    pub exported_at: Option<String>,
    pub forwards: Vec<ForwardConfig>,
    /// F19: v1 backups have NO `groups` key → defaults to empty.
    #[serde(default)]
    pub groups: Vec<TunnelGroup>,
}

impl BackupFile {
    /// Build an export payload from the current forwards + groups. Every
    /// `hasStoredPassword` is forced false so the recipient sees no secret
    /// (AGENTS §8); no password field exists on the wire in the first place.
    pub fn export(forwards: &[ForwardConfig], groups: &[TunnelGroup], now: String) -> Self {
        let forwards = forwards
            .iter()
            .cloned()
            .map(|mut f| {
                f.has_stored_password = false;
                f
            })
            .collect();
        Self {
            version: BACKUP_VERSION,
            exported_at: Some(now),
            forwards,
            groups: groups.to_vec(),
        }
    }

    /// Serialize to pretty JSON for `export_backup`.
    pub fn to_json(&self) -> Result<String, AppError> {
        serde_json::to_string_pretty(self).map_err(AppError::from)
    }
}

/// Lenient parse + version validation of a backup file (v1 or v2).
///
/// - Rejects `version > BACKUP_VERSION` with a clear error (matches v1, which
///   rejected `> 1`).
/// - Tolerates a v1 backup: missing `groups` → `[]`; a stray `sshPassword` on a
///   forward entry is ignored (never imported as a secret).
/// - Forces `hasStoredPassword = false` on every imported forward — a backup is
///   password-free, so the user must re-enter passwords after import.
pub fn parse_backup(bytes: &[u8]) -> Result<BackupFile, AppError> {
    let mut backup: BackupFile = serde_json::from_slice(bytes)
        .map_err(|e| AppError::Backup(format!("invalid backup: malformed JSON ({e})")))?;

    if backup.version > BACKUP_VERSION {
        return Err(AppError::Backup(format!(
            "backup version {} is newer than this app supports (max v{}). Please update Tunnel Pilot.",
            backup.version, BACKUP_VERSION
        )));
    }

    for f in &mut backup.forwards {
        // Backups carry no secrets; the recipient must re-enter passwords.
        f.has_stored_password = false;
    }

    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v1_backup_without_groups_and_ignores_ssh_password() {
        // A real v1 export shape: version 1, no `groups`, and (defensively) a
        // legacy `sshPassword` on the entry.
        let v1 = r#"{
            "version": 1,
            "exportedAt": "2024-01-01T00:00:00.000Z",
            "forwards": [
                {
                    "id": "v1-fwd",
                    "name": "Legacy",
                    "sshHost": "h",
                    "sshUsername": "u",
                    "sshPassword": "leaked-if-imported",
                    "identityFilePath": null,
                    "localPort": 1234,
                    "remoteHost": "r",
                    "remotePort": 5678,
                    "keepAliveIntervalSec": 30,
                    "keepAliveMaxCount": 5
                }
            ]
        }"#;

        let backup = parse_backup(v1.as_bytes()).expect("lenient v1 parse");
        assert_eq!(backup.version, 1);
        assert!(backup.groups.is_empty(), "missing groups defaults to []");
        assert_eq!(backup.forwards.len(), 1);

        let f = &backup.forwards[0];
        assert_eq!(f.id, "v1-fwd");
        assert_eq!(f.ssh_port, 22, "v2 default applied to a v1 entry");
        assert!(
            !f.has_stored_password,
            "backups never carry a stored password"
        );
        assert!(f.tags.is_empty() && f.group_id.is_none());

        // The legacy sshPassword can never resurface — it is not a field on
        // ForwardConfig, so re-serializing the parsed backup drops it.
        let round = serde_json::to_string(f).expect("reserialize");
        assert!(!round.contains("leaked-if-imported"));
        assert!(!round.contains("sshPassword"));
    }

    #[test]
    fn rejects_backup_version_newer_than_current() {
        let future = br#"{ "version": 3, "forwards": [] }"#;
        let err = parse_backup(future).expect_err("must reject");
        assert!(matches!(err, AppError::Backup(_)));
    }

    #[test]
    fn export_strips_stored_password_flag() {
        let f = ForwardConfig {
            id: "x".into(),
            name: "n".into(),
            ssh_host: "h".into(),
            ssh_port: 22,
            ssh_username: "u".into(),
            identity_file_path: None,
            has_stored_password: true,
            local_bind_address: "127.0.0.1".into(),
            local_port: 1,
            remote_host: "r".into(),
            remote_port: 2,
            keep_alive_interval_sec: 30,
            keep_alive_max_count: 5,
            group_id: None,
            tags: vec![],
        };
        let backup = BackupFile::export(&[f], &[], "now".into());
        assert_eq!(backup.version, BACKUP_VERSION);
        assert!(!backup.forwards[0].has_stored_password);
        let json = backup.to_json().expect("json");
        assert!(!json.contains("sshPassword"));
    }
}
