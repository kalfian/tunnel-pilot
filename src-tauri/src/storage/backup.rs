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
use crate::state::models::{ForwardConfig, TunnelGroup, Variable};

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

/// The variables after an import, plus the secret variables whose stored
/// values must be deleted because the import dropped them (or made them
/// plain). Pure — unit-tested (spec 04 §11, 03 §24).
///
/// - **Replace**: the backup's variables. A secret one keeps the value already
///   stored on this machine under the same name (a backup never carries it).
/// - **Merge**: current variables, plus backup variables whose name (ignoring
///   case) is new.
pub fn plan_variable_import(
    current: &[Variable],
    incoming: Vec<Variable>,
    mode: ImportMode,
) -> (Vec<Variable>, Vec<String>) {
    match mode {
        ImportMode::Replace => {
            let planned: Vec<Variable> = incoming
                .into_iter()
                .map(|mut v| {
                    if v.secret {
                        v.has_stored_value = current
                            .iter()
                            .any(|c| c.name == v.name && c.secret && c.has_stored_value);
                    }
                    v
                })
                .collect();
            let orphaned = current
                .iter()
                .filter(|c| c.secret && c.has_stored_value)
                .filter(|c| !planned.iter().any(|v| v.name == c.name && v.secret))
                .map(|c| c.name.clone())
                .collect();
            (planned, orphaned)
        }
        ImportMode::Merge => {
            let mut merged = current.to_vec();
            for v in incoming {
                if !merged.iter().any(|c| c.name.eq_ignore_ascii_case(&v.name)) {
                    merged.push(v);
                }
            }
            (merged, Vec::new())
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
    /// Variables (spec 03 §24): plain values, and secret ones by NAME only —
    /// a backup never holds a secret value. Absent in older backups.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variables: Vec<Variable>,
}

impl BackupFile {
    /// Build an export payload from the current forwards + groups. Every
    /// `hasStoredPassword` is forced false so the recipient sees no secret
    /// (AGENTS §8); no password field exists on the wire in the first place.
    pub fn export(
        forwards: &[ForwardConfig],
        groups: &[TunnelGroup],
        variables: &[Variable],
        now: String,
    ) -> Self {
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
            variables: variables.iter().cloned().map(strip_secret).collect(),
        }
    }

    /// Serialize to pretty JSON for `export_backup`.
    pub fn to_json(&self) -> Result<String, AppError> {
        serde_json::to_string_pretty(self).map_err(AppError::from)
    }
}

/// A variable as it may appear in a backup: a secret one by name only.
fn strip_secret(mut v: Variable) -> Variable {
    if v.secret {
        v.value = String::new();
    }
    v.has_stored_value = false;
    v
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

    // A hand-edited backup could put a value on a secret variable: drop it so
    // it can never land in the config file. Malformed entries are skipped (a
    // forward that used one reports the missing variable when it connects).
    let mut seen: Vec<String> = Vec::new();
    backup.variables = std::mem::take(&mut backup.variables)
        .into_iter()
        .map(strip_secret)
        .filter(|v| {
            let valid = crate::variables::validate_name(&v.name).is_ok()
                && (v.secret || crate::variables::validate_plain_value(&v.name, &v.value).is_ok());
            let fresh = !seen.iter().any(|n| n.eq_ignore_ascii_case(&v.name));
            if valid && fresh {
                seen.push(v.name.clone());
                true
            } else {
                tracing::warn!(variable = %v.name, "skipping an invalid or duplicate variable in the backup");
                false
            }
        })
        .collect();

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
            host_key: None,
            password_variable: None,
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
            variables: vec![],
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
            variables: vec![],
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
            host_key: None,
            password_variable: None,
        };
        let backup = BackupFile::export(&[f], &[], &[], "now".into());
        assert_eq!(backup.version, BACKUP_VERSION);
        assert!(!backup.forwards[0].has_stored_password);
        let json = backup.to_json().expect("json");
        assert!(!json.contains("sshPassword"));
    }

    fn var(name: &str, secret: bool, value: &str, stored: bool) -> Variable {
        Variable {
            name: name.into(),
            secret,
            value: value.into(),
            has_stored_value: stored,
        }
    }

    #[test]
    fn export_keeps_plain_values_and_only_names_of_secrets() {
        let backup = BackupFile::export(
            &[],
            &[],
            &[
                var("USER", false, "kalfian", false),
                var("PASS", true, "", true),
            ],
            "now".into(),
        );
        let json = backup.to_json().expect("json");
        assert!(json.contains("kalfian"));
        let pass = backup
            .variables
            .iter()
            .find(|v| v.name == "PASS")
            .expect("pass");
        assert!(pass.secret && pass.value.is_empty() && !pass.has_stored_value);
    }

    #[test]
    fn parse_drops_smuggled_secret_values_and_invalid_entries() {
        let raw = r#"{
            "version": 2,
            "forwards": [],
            "variables": [
                { "name": "PASS", "secret": true, "value": "leaked", "hasStoredValue": true },
                { "name": "USER", "value": "u" },
                { "name": "user", "value": "dup" },
                { "name": "bad-name", "value": "x" },
                { "name": "EMPTY", "value": "" }
            ]
        }"#;
        let backup = parse_backup(raw.as_bytes()).expect("parse");
        let names: Vec<&str> = backup.variables.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec!["PASS", "USER"]);
        let pass = &backup.variables[0];
        assert!(pass.value.is_empty() && !pass.has_stored_value);
        assert!(!serde_json::to_string(&backup.variables)
            .expect("json")
            .contains("leaked"));
    }

    #[test]
    fn older_backups_without_variables_still_parse() {
        let raw = r#"{ "version": 2, "forwards": [], "groups": [] }"#;
        assert!(parse_backup(raw.as_bytes())
            .expect("parse")
            .variables
            .is_empty());
    }

    #[test]
    fn replace_keeps_local_secrets_by_name_and_reports_orphans() {
        let current = vec![
            var("PASS", true, "", true),
            var("OLD", true, "", true),
            var("USER", false, "a", false),
        ];
        let incoming = vec![var("PASS", true, "", false), var("USER", false, "b", false)];
        let (planned, orphaned) = plan_variable_import(&current, incoming, ImportMode::Replace);
        assert_eq!(planned.len(), 2);
        assert!(
            planned[0].has_stored_value,
            "PASS keeps this machine's secret"
        );
        assert_eq!(planned[1].value, "b");
        assert_eq!(orphaned, vec!["OLD".to_string()]);
    }

    #[test]
    fn merge_adds_only_new_names() {
        let current = vec![var("USER", false, "mine", false)];
        let incoming = vec![
            var("user", false, "theirs", false),
            var("PASS", true, "", false),
        ];
        let (planned, orphaned) = plan_variable_import(&current, incoming, ImportMode::Merge);
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].value, "mine");
        assert_eq!(planned[1].name, "PASS");
        assert!(orphaned.is_empty());
    }
}
