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

/// How an import merges with the existing config (spec 02 §6.5, 04 §11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportMode {
    /// Clear existing forwards+groups, insert the backup's.
    Replace,
    /// Append entries not already present; skip duplicates.
    Merge,
}

/// Outcome of an import (spec 02 §6.5): how many forwards were added, how many
/// were skipped as duplicates (merge only), and whether it replaced everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub replaced: bool,
}

/// The natural dedupe key for a forward on a MERGE (spec 04 §11): a forward is a
/// duplicate of an existing one if the ids match OR this identity tuple matches.
fn natural_key(f: &ForwardConfig) -> (String, String, u16, u16, String, u16) {
    (
        f.name.clone(),
        f.ssh_host.clone(),
        f.ssh_port,
        f.local_port,
        f.remote_host.clone(),
        f.remote_port,
    )
}

/// Compute the post-import `(forwards, groups, result)` from the current state,
/// the parsed `backup`, and the `mode` — pure so replace/merge semantics are
/// unit-testable without `AppState` (spec 04 §11).
///
/// - **Replace**: result forwards/groups ARE the backup's; `imported` = count,
///   `skipped` = 0, `replaced` = true.
/// - **Merge**: keep current, append each backup forward whose id or natural key
///   is not already present (dups counted in `skipped`); append backup groups
///   whose id is new. `replaced` = false.
///
/// Imported forwards always have `has_stored_password = false` (parse forced it).
pub fn plan_import(
    current_forwards: &[ForwardConfig],
    current_groups: &[TunnelGroup],
    backup: BackupFile,
    mode: ImportMode,
) -> (Vec<ForwardConfig>, Vec<TunnelGroup>, ImportResult) {
    match mode {
        ImportMode::Replace => {
            let imported = backup.forwards.len();
            let result = ImportResult {
                imported,
                skipped: 0,
                replaced: true,
            };
            (backup.forwards, backup.groups, result)
        }
        ImportMode::Merge => {
            let mut forwards = current_forwards.to_vec();
            let mut imported = 0usize;
            let mut skipped = 0usize;
            for f in backup.forwards {
                let dup = forwards
                    .iter()
                    .any(|e| e.id == f.id || natural_key(e) == natural_key(&f));
                if dup {
                    skipped += 1;
                } else {
                    forwards.push(f);
                    imported += 1;
                }
            }

            let mut groups = current_groups.to_vec();
            for g in backup.groups {
                if !groups.iter().any(|e| e.id == g.id) {
                    groups.push(g);
                }
            }

            let result = ImportResult {
                imported,
                skipped,
                replaced: false,
            };
            (forwards, groups, result)
        }
    }
}

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

    fn fwd(id: &str, name: &str, local_port: u16) -> ForwardConfig {
        ForwardConfig {
            id: id.into(),
            name: name.into(),
            ssh_host: "h".into(),
            ssh_port: 22,
            ssh_username: "u".into(),
            identity_file_path: None,
            has_stored_password: false,
            local_bind_address: "127.0.0.1".into(),
            local_port,
            remote_host: "r".into(),
            remote_port: 5432,
            keep_alive_interval_sec: 30,
            keep_alive_max_count: 5,
            group_id: None,
            tags: vec![],
        }
    }

    fn grp(id: &str) -> TunnelGroup {
        TunnelGroup {
            id: id.into(),
            name: format!("g-{id}"),
            color: None,
            order: 0,
            collapsed: false,
        }
    }

    #[test]
    fn replace_import_swaps_everything() {
        let current_f = vec![fwd("keep", "Keep", 1)];
        let current_g = vec![grp("old")];
        let backup = BackupFile {
            version: 2,
            exported_at: None,
            forwards: vec![fwd("new1", "New1", 2), fwd("new2", "New2", 3)],
            groups: vec![grp("newgrp")],
        };
        let (forwards, groups, result) =
            plan_import(&current_f, &current_g, backup, ImportMode::Replace);

        assert!(result.replaced);
        assert_eq!(result.imported, 2);
        assert_eq!(result.skipped, 0);
        assert_eq!(
            forwards.iter().map(|f| f.id.clone()).collect::<Vec<_>>(),
            ["new1", "new2"]
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].id, "newgrp");
    }

    #[test]
    fn merge_import_appends_new_and_skips_duplicates() {
        // Existing: one forward. Backup: a same-id dup, a same natural-key dup
        // (different id), and one genuinely new forward.
        let current_f = vec![fwd("a", "Alpha", 10)];
        let current_g = vec![grp("g1")];
        let backup = BackupFile {
            version: 2,
            exported_at: None,
            forwards: vec![
                fwd("a", "Alpha", 10),              // duplicate by id
                fwd("b-different-id", "Alpha", 10), // duplicate by natural key
                fwd("c", "Charlie", 11),            // new
            ],
            groups: vec![grp("g1"), grp("g2")], // g1 dup, g2 new
        };
        let (forwards, groups, result) =
            plan_import(&current_f, &current_g, backup, ImportMode::Merge);

        assert!(!result.replaced);
        assert_eq!(result.imported, 1, "only Charlie is new");
        assert_eq!(result.skipped, 2, "id dup + natural-key dup");
        assert_eq!(forwards.len(), 2);
        assert!(forwards.iter().any(|f| f.id == "c"));
        // Groups: g1 kept once, g2 appended.
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().any(|g| g.id == "g2"));
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
