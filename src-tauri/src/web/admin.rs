//! The web UI admin password (spec 02 §10, 03 §22).
//!
//! Stored as an argon2id PHC string in `<configDir>/web_admin.json`, written
//! atomically with mode `0600` from the start (never world-readable, even
//! briefly). The plaintext password is never stored, logged or echoed. Changing
//! it signs every web session out: a session remembers the hash it was created
//! under, and a different hash on disk invalidates it (`auth.rs`).

use std::io::Write;
use std::path::{Path, PathBuf};

use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use serde::{Deserialize, Serialize};

use crate::cli::args::WebAction;
use crate::cli::client::{EXIT_INTERNAL, EXIT_OK, EXIT_USAGE};
use crate::error::AppError;

/// File name of the admin password hash inside the config dir.
pub const ADMIN_FILE: &str = "web_admin.json";

/// Minimum admin password length. The web UI can be reachable from a network,
/// so a short password is refused outright.
pub const MIN_PASSWORD_LEN: usize = 10;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdminFile {
    schema_version: u32,
    /// argon2id PHC string (`$argon2id$v=19$...`). Never the password.
    hash: String,
}

/// `<configDir>/web_admin.json`.
pub fn admin_path(config_dir: &Path) -> PathBuf {
    config_dir.join(ADMIN_FILE)
}

/// The stored hash, `Ok(None)` when no password has been set.
pub fn load_hash(config_dir: &Path) -> Result<Option<String>, AppError> {
    match std::fs::read(admin_path(config_dir)) {
        Ok(bytes) => {
            let file: AdminFile = serde_json::from_slice(&bytes)?;
            Ok(Some(file.hash))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Whether a usable admin password is configured.
pub fn password_is_set(config_dir: &Path) -> bool {
    matches!(load_hash(config_dir), Ok(Some(_)))
}

/// Check a password against the policy (no storage). Pure.
pub fn validate_password(password: &str) -> Result<(), AppError> {
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(AppError::InvalidInput(format!(
            "the admin password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if password.trim() != password {
        return Err(AppError::InvalidInput(
            "the admin password must not start or end with whitespace".into(),
        ));
    }
    Ok(())
}

/// Hash `password` with argon2id and a fresh random salt.
pub fn hash_password(password: &str) -> Result<String, AppError> {
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt)
        .map_err(|e| AppError::Internal(format!("no OS randomness for the salt: {e}")))?;
    let salt = SaltString::encode_b64(&salt)
        .map_err(|e| AppError::Internal(format!("salt encoding failed: {e}")))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AppError::Internal(format!("password hashing failed: {e}")))
}

/// Constant-time verification (argon2 compares in constant time). A corrupt
/// hash verifies as `false`, never as success.
pub fn verify_password(hash: &str, password: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

/// Validate, hash and store a new admin password (atomic, `0600`).
pub fn set_password(config_dir: &Path, password: &str) -> Result<(), AppError> {
    validate_password(password)?;
    let hash = hash_password(password)?;
    let body = serde_json::to_vec_pretty(&AdminFile {
        schema_version: 1,
        hash,
    })?;
    write_private(&admin_path(config_dir), &body)
}

/// Write `bytes` to `path` via a `0600` temp sibling + rename.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let dir = path
        .parent()
        .ok_or_else(|| AppError::Internal("admin file has no parent dir".into()))?;
    create_private_dir(dir)?;
    let tmp = path.with_extension("json.tmp");
    // A leftover temp file could carry looser permissions; never reuse it.
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn create_private_dir(dir: &Path) -> Result<(), AppError> {
    if dir.exists() {
        return Ok(());
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)?;
    Ok(())
}

/// `tunnel-pilot-cli web set-password | status`.
pub fn run(action: WebAction) -> i32 {
    let Some(config_dir) = crate::cli::default_config_dir() else {
        eprintln!(
            "error: could not resolve the config dir; set {}",
            crate::cli::CONFIG_DIR_ENV
        );
        return EXIT_INTERNAL;
    };
    match action {
        WebAction::Status => {
            if password_is_set(&config_dir) {
                println!(
                    "web admin password: set ({})",
                    admin_path(&config_dir).display()
                );
            } else {
                println!("web admin password: NOT set — run: tunnel-pilot-cli web set-password");
            }
            EXIT_OK
        }
        WebAction::SetPassword { stdin } => {
            let password = match read_new_password(stdin) {
                Ok(p) => p,
                Err(code) => return code,
            };
            match set_password(&config_dir, &password) {
                Ok(()) => {
                    println!("Web admin password set. Every open web session is now signed out.");
                    EXIT_OK
                }
                Err(AppError::InvalidInput(msg)) => {
                    eprintln!("error: {msg}");
                    EXIT_USAGE
                }
                Err(e) => {
                    eprintln!("error: could not store the admin password: {e}");
                    EXIT_INTERNAL
                }
            }
        }
    }
}

/// Prompt twice (echo off) or read one stdin line.
fn read_new_password(stdin: bool) -> Result<String, i32> {
    if stdin {
        let mut line = String::new();
        if let Err(e) = std::io::stdin().read_line(&mut line) {
            eprintln!("error: could not read the password from stdin: {e}");
            return Err(EXIT_INTERNAL);
        }
        return Ok(line.trim_end_matches(['\n', '\r']).to_string());
    }
    let prompt = |label: &str| {
        rpassword::prompt_password(label).map_err(|e| {
            eprintln!("error: cannot prompt ({e}); use --password-stdin instead");
            EXIT_USAGE
        })
    };
    let first = prompt("New web admin password: ")?;
    let second = prompt("Repeat it: ")?;
    if first != second {
        eprintln!("error: the passwords do not match");
        return Err(EXIT_USAGE);
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_rejects_short_and_padded_passwords() {
        assert!(validate_password("short").is_err());
        assert!(validate_password(" leading-space-pass").is_err());
        assert!(validate_password("long-enough-pass").is_ok());
    }

    #[test]
    fn set_then_verify_and_file_is_private() {
        let dir = tempfile::tempdir().expect("tmp");
        let cfg = dir.path().join("cfg");
        assert!(!password_is_set(&cfg));
        set_password(&cfg, "correct horse battery").expect("set");
        assert!(password_is_set(&cfg));

        let hash = load_hash(&cfg).expect("load").expect("some");
        assert!(hash.starts_with("$argon2id$"));
        assert!(verify_password(&hash, "correct horse battery"));
        assert!(!verify_password(&hash, "correct horse batterY"));
        assert!(!verify_password("not-a-phc-string", "anything"));

        let raw = std::fs::read_to_string(admin_path(&cfg)).expect("read");
        assert!(
            !raw.contains("correct horse"),
            "plaintext must never be stored"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(admin_path(&cfg))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
            let dir_mode = std::fs::metadata(&cfg).expect("meta").permissions().mode();
            assert_eq!(dir_mode & 0o777, 0o700);
        }

        // A new password produces a different hash (fresh salt).
        set_password(&cfg, "correct horse battery").expect("reset");
        assert_ne!(load_hash(&cfg).expect("load").expect("some"), hash);
    }
}
