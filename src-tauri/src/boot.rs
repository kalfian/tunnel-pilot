//! Boot steps shared by the desktop app (`lib.rs::run`) and the headless
//! daemon (`daemon.rs`), so both owners of `AppState` load the same config the
//! same way (spec 03 §7, §21).

use crate::credentials::CredentialStore;
use crate::storage::config_file::{ConfigDocument, ConfigStore};

/// First-run v1→v2 migration (hardcoded per-OS v1 probe, plaintext passwords →
/// keychain), then load the v2 document. Bad config never crashes the app
/// (spec 03 §7): on error we log and start with defaults.
pub async fn load_document(
    config_store: &ConfigStore,
    credentials: &CredentialStore,
) -> ConfigDocument {
    if let Err(e) = crate::storage::migration::migrate_if_needed(config_store, credentials).await {
        tracing::error!(error = %e, "v1→v2 migration failed; continuing");
    }
    // (A missing keychain is already logged once by `CredentialStore` itself.)
    config_store.load().await.unwrap_or_else(|e| {
        tracing::error!(error = %e, "failed to load config; starting with defaults");
        ConfigDocument::default()
    })
}
