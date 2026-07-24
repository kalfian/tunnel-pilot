//! OS-keychain backend behind a small trait so the routing logic in
//! [`super::CredentialStore`] is unit-testable without a live keychain.
//!
//! Real impl: the `keyring` crate (per-target native backend pinned in
//! `Cargo.toml`, F9). macOS Keychain / Windows Credential Manager / Linux
//! Secret Service. `KC_SERVICE` is the stable service string; the account is
//! the forward id (spec 04 §10).
//!
//! Security: keyring's own errors never contain the stored secret, so mapping
//! them to `AppError::Credential(String)` is safe. We never log the secret.

use keyring::{Entry, Error as KeyringError};

use crate::error::AppError;

/// Stable, deliberate service string (spec 03 §9, 04 §10). It intentionally
/// does NOT match the bundle id so keychain entries survive a future bundle-id
/// change. Do not change once shipped.
pub const KC_SERVICE: &str = "tunnel-pilot";

/// Sentinel account used only by the boot-time availability probe. Namespaced
/// so it can never collide with a real forward id (uuid).
const PROBE_ACCOUNT: &str = "__probe__.keychain_available";

/// A password store keyed by account name. Abstracts the real OS keychain so
/// [`super::CredentialStore`] routing can be tested against an in-memory mock.
pub trait KeychainBackend: Send + Sync {
    /// Store (or replace) the secret for `account`.
    fn set(&self, account: &str, secret: &str) -> Result<(), AppError>;
    /// Fetch the secret for `account`; `Ok(None)` if there is no entry.
    fn get(&self, account: &str) -> Result<Option<String>, AppError>;
    /// Delete the secret for `account`; missing entry is a no-op.
    fn delete(&self, account: &str) -> Result<(), AppError>;
    /// Probe whether this backend actually works right now (set+get+delete a
    /// sentinel). On headless Linux with no Secret Service this returns false.
    fn probe(&self) -> bool;
}

/// The production backend: the `keyring` crate with the per-target native
/// backend selected in `Cargo.toml`.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringBackend;

impl KeyringBackend {
    fn entry(account: &str) -> Result<Entry, AppError> {
        Entry::new(KC_SERVICE, account)
            .map_err(|e| AppError::Credential(format!("keychain open failed: {e}")))
    }
}

impl KeychainBackend for KeyringBackend {
    fn set(&self, account: &str, secret: &str) -> Result<(), AppError> {
        Self::entry(account)?
            .set_password(secret)
            .map_err(|e| AppError::Credential(format!("keychain write failed: {e}")))
    }

    fn get(&self, account: &str) -> Result<Option<String>, AppError> {
        match Self::entry(account)?.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(KeyringError::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Credential(format!("keychain read failed: {e}"))),
        }
    }

    fn delete(&self, account: &str) -> Result<(), AppError> {
        match Self::entry(account)?.delete_credential() {
            Ok(()) => Ok(()),
            Err(KeyringError::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Credential(format!("keychain delete failed: {e}"))),
        }
    }

    fn probe(&self) -> bool {
        // A round-trip on a sentinel account is the only reliable feature-test:
        // the backend may link fine yet fail at runtime (locked store, no
        // D-Bus). The sentinel value is not a secret.
        let sentinel = "1";
        if self.set(PROBE_ACCOUNT, sentinel).is_err() {
            return false;
        }
        let ok = matches!(self.get(PROBE_ACCOUNT), Ok(Some(v)) if v == sentinel);
        // Always attempt cleanup regardless of the read result.
        let _ = self.delete(PROBE_ACCOUNT);
        ok
    }
}
