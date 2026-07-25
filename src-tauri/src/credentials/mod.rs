//! Credential storage: OS keychain first, plaintext fallback file second.
//!
//! Tries the OS keychain (macOS Keychain / Windows Credential Manager / Linux
//! Secret Service) via the `keyring` crate. When the keychain is unavailable
//! (e.g. headless Linux, no D-Bus), secrets go to a **separate** fallback file
//! and the UI shows a warning driven by `keychain_available() == false`. The
//! main config JSON only ever holds `hasStoredPassword`, never the secret
//! (spec 03 §9, 04 §10; AGENTS §8 — non-negotiable).
//!
//! Security: passwords are never logged, emitted over IPC, embedded in
//! `copy_ssh_command`, or written to backups. Log/trace lines reference a
//! forward only by its id (a uuid), never by its secret.

mod backend;
mod fallback;

use std::path::{Path, PathBuf};

pub use backend::KC_SERVICE;
use backend::{InMemoryBackend, KeychainBackend, KeyringBackend, NullBackend};
use fallback::FallbackStore;

use crate::error::AppError;

/// Canonical name of the fallback secrets file (spec 04 §10). Lives in the app-
/// support dir, kept separate from `tunnel_pilot_config.json`.
pub const SECRETS_FILE_NAME: &str = "tunnel_pilot_secrets.json";

/// Whether the OS keychain works right now. Feature-detects with a sentinel
/// round-trip (set+get+delete). On headless Linux (no Secret Service) this is
/// false. Exposed as a free function to match the spec 03 §9 signature; prefer
/// [`CredentialStore::keychain_available`] in app code, which caches the result
/// at boot instead of probing on every call.
pub fn keychain_available() -> bool {
    KeyringBackend.probe()
}

/// Password store keyed by forward id, with keychain-first / fallback-file
/// routing. Construct once at boot (the keychain availability probe is cached);
/// share it via `AppState`.
pub struct CredentialStore {
    keychain: Box<dyn KeychainBackend>,
    fallback: FallbackStore,
    keychain_available: bool,
}

impl CredentialStore {
    /// Production constructor: probe the real OS keychain once and cache the
    /// result. `secrets_file` is the fallback path (app-support dir +
    /// [`SECRETS_FILE_NAME`]).
    pub fn new(secrets_file: PathBuf) -> Self {
        Self::with_backend(Box::new(KeyringBackend), secrets_file)
    }

    /// Build from the app-support directory, appending [`SECRETS_FILE_NAME`].
    pub fn from_app_dir(app_support_dir: &Path) -> Self {
        Self::new(app_support_dir.join(SECRETS_FILE_NAME))
    }

    /// Fully in-memory store (no OS keychain, no disk). `keychain_available()`
    /// reports true so the keychain route is exercised. For headless
    /// `AppState` (engine tests) and migration tests only — never production.
    pub fn in_memory() -> Self {
        Self {
            keychain: Box::new(InMemoryBackend::default()),
            fallback: FallbackStore::new(PathBuf::new()),
            keychain_available: true,
        }
    }

    /// Fallback-only store: the keychain is forced unavailable so every secret
    /// is routed to the plaintext `secrets_file`. Used on headless Linux (no
    /// Secret Service) and to exercise the fallback route deterministically.
    pub fn fallback_only(secrets_file: PathBuf) -> Self {
        Self::with_backend(Box::new(NullBackend), secrets_file)
    }

    /// Wire an explicit backend (probed once) + fallback path. `pub(crate)` so
    /// tests can inject a mock keychain without a live OS store.
    pub(crate) fn with_backend(keychain: Box<dyn KeychainBackend>, secrets_file: PathBuf) -> Self {
        let keychain_available = keychain.probe();
        if !keychain_available {
            tracing::warn!(
                secrets_file = %secrets_file.display(),
                "OS keychain unavailable; storing SSH passwords in plaintext fallback file"
            );
        }
        Self {
            keychain,
            fallback: FallbackStore::new(secrets_file),
            keychain_available,
        }
    }

    /// Cached keychain availability (propagated to the UI for the warning).
    pub fn keychain_available(&self) -> bool {
        self.keychain_available
    }

    /// Path of the fallback secrets file (diagnostics only — never contents).
    pub fn fallback_path(&self) -> &Path {
        self.fallback.path()
    }

    /// Store the password for forward `id`. Routes to the keychain when
    /// available, else the fallback file. The secret never touches the main
    /// config or any log line.
    pub fn set_password(&self, id: &str, secret: &str) -> Result<(), AppError> {
        if self.keychain_available {
            tracing::debug!(forward_id = %id, "storing password in OS keychain");
            self.keychain.set(id, secret)
        } else {
            tracing::debug!(forward_id = %id, "storing password in fallback secrets file");
            self.fallback.set(id, secret)
        }
    }

    /// Fetch the password for forward `id`. Keychain first; if absent there,
    /// falls through to the fallback file (covers a store that only became
    /// available after secrets were written to disk). `Ok(None)` if nowhere.
    pub fn get_password(&self, id: &str) -> Result<Option<String>, AppError> {
        if self.keychain_available {
            if let Some(pw) = self.keychain.get(id)? {
                return Ok(Some(pw));
            }
        }
        self.fallback.get(id)
    }

    /// Delete the password for forward `id` from BOTH stores (best-effort) so a
    /// forward deletion never leaves an orphaned secret behind. Idempotent.
    pub fn delete_password(&self, id: &str) -> Result<(), AppError> {
        // Delete from the keychain even if currently "unavailable": the store
        // may have been available on a prior run. Errors other than the
        // absence of an entry are surfaced.
        let keychain_result = self.keychain.delete(id);
        let fallback_result = self.fallback.delete(id);
        keychain_result.and(fallback_result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory keychain double. `available` controls what `probe()` reports,
    /// so we can exercise BOTH routing paths without a live OS keychain.
    struct MockKeychain {
        available: bool,
        store: Mutex<HashMap<String, String>>,
    }

    impl MockKeychain {
        fn new(available: bool) -> Self {
            Self {
                available,
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    impl KeychainBackend for MockKeychain {
        fn set(&self, account: &str, secret: &str) -> Result<(), AppError> {
            self.store
                .lock()
                .expect("mock lock")
                .insert(account.to_string(), secret.to_string());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, AppError> {
            Ok(self.store.lock().expect("mock lock").get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<(), AppError> {
            self.store.lock().expect("mock lock").remove(account);
            Ok(())
        }
        fn probe(&self) -> bool {
            self.available
        }
    }

    fn temp_secrets_path() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(SECRETS_FILE_NAME);
        (dir, path)
    }

    // ---- Fallback path (keychain UNAVAILABLE) — must pass unconditionally ----

    #[test]
    fn fallback_roundtrip_set_get_delete() {
        let (_dir, path) = temp_secrets_path();
        let store = CredentialStore::with_backend(Box::new(MockKeychain::new(false)), path);

        assert!(!store.keychain_available());
        assert_eq!(store.get_password("f1").expect("get"), None);

        store.set_password("f1", "hunter2").expect("set");
        assert_eq!(
            store.get_password("f1").expect("get"),
            Some("hunter2".to_string())
        );

        store.delete_password("f1").expect("delete");
        assert_eq!(store.get_password("f1").expect("get"), None);
    }

    #[test]
    fn fallback_isolates_secrets_per_forward() {
        let (_dir, path) = temp_secrets_path();
        let store = CredentialStore::with_backend(Box::new(MockKeychain::new(false)), path);

        store.set_password("a", "secret-a").expect("set a");
        store.set_password("b", "secret-b").expect("set b");
        store.delete_password("a").expect("delete a");

        assert_eq!(store.get_password("a").expect("get a"), None);
        assert_eq!(
            store.get_password("b").expect("get b"),
            Some("secret-b".to_string())
        );
    }

    #[test]
    fn delete_missing_is_idempotent() {
        let (_dir, path) = temp_secrets_path();
        let store = CredentialStore::with_backend(Box::new(MockKeychain::new(false)), path);
        // Deleting a never-stored id must not error.
        store.delete_password("nope").expect("idempotent delete");
    }

    // ---- Routing: keychain AVAILABLE must NOT write the fallback file ----

    #[test]
    fn keychain_available_does_not_write_fallback_file() {
        let (_dir, path) = temp_secrets_path();
        let store = CredentialStore::with_backend(Box::new(MockKeychain::new(true)), path.clone());

        assert!(store.keychain_available());
        store.set_password("f1", "topsecret").expect("set");

        // The secret round-trips through the (mock) keychain...
        assert_eq!(
            store.get_password("f1").expect("get"),
            Some("topsecret".to_string())
        );
        // ...and the on-disk fallback file was never created.
        assert!(
            !path.exists(),
            "fallback secrets file must not exist when keychain is available"
        );
    }

    // ---- Security: the secret lands ONLY in the fallback file, and the ----
    // ---- file never leaks it under any key but secrets[<id>].          ----

    #[test]
    fn secret_only_written_to_fallback_file_and_nowhere_else() {
        let (_dir, path) = temp_secrets_path();
        let store = CredentialStore::with_backend(Box::new(MockKeychain::new(false)), path.clone());

        let secret = "s3cr3t-do-not-leak";
        store.set_password("fwd-123", secret).expect("set");

        // The only artifact is the fallback file, and its structure is exactly
        // { schemaVersion, secrets: { "<id>": "<secret>" } }.
        let raw = std::fs::read_to_string(&path).expect("read secrets file");
        let json: serde_json::Value = serde_json::from_str(&raw).expect("parse");
        assert_eq!(json["schemaVersion"], 1);
        assert_eq!(json["secrets"]["fwd-123"], secret);
        // No stray top-level keys beyond the two documented ones.
        let obj = json.as_object().expect("object");
        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("schemaVersion") && obj.contains_key("secrets"));
    }

    #[test]
    fn fallback_get_falls_through_when_keychain_available_but_empty() {
        // Simulate: secrets were written to the fallback file on a headless
        // run, then the keychain became available. `get` should still find the
        // on-disk secret (keychain miss → fallback read).
        let (_dir, path) = temp_secrets_path();
        std::fs::write(
            &path,
            r#"{"schemaVersion":1,"secrets":{"legacy":"old-pw"}}"#,
        )
        .expect("seed secrets file");

        let store = CredentialStore::with_backend(Box::new(MockKeychain::new(true)), path);
        assert_eq!(
            store.get_password("legacy").expect("get"),
            Some("old-pw".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn fallback_file_is_owner_only_0600() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, path) = temp_secrets_path();
        let store = CredentialStore::with_backend(Box::new(MockKeychain::new(false)), path.clone());
        store.set_password("f1", "pw").expect("set");
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // ---- Real OS keychain roundtrip: gated (needs a live, unlocked store) ----

    #[test]
    #[ignore = "requires a live, unlocked OS keychain (run: cargo test -- --ignored)"]
    fn real_keychain_roundtrip() {
        if !keychain_available() {
            eprintln!("keychain unavailable in this environment; skipping");
            return;
        }
        let (_dir, path) = temp_secrets_path();
        let store = CredentialStore::new(path.clone());
        assert!(store.keychain_available());

        let id = format!("test-{}", uuid::Uuid::new_v4());
        store.set_password(&id, "roundtrip-secret").expect("set");
        assert_eq!(
            store.get_password(&id).expect("get"),
            Some("roundtrip-secret".to_string())
        );
        store.delete_password(&id).expect("delete");
        assert_eq!(store.get_password(&id).expect("get"), None);
        // Even on the keychain path, no fallback file should ever be created.
        assert!(!path.exists());
    }
}
