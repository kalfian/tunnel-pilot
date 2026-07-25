//! russh client `Handler`, session config, connect, and authentication
//! (spec 03 §§1,2).
//!
//! Liveness is owned by russh keepalive: [`build_config`] sets
//! `keepalive_interval`/`keepalive_max`, so when the peer misses
//! `keepalive_max` keepalives russh's session task exits — observed by the
//! supervisor via `Handle::is_closed()` (F7, F16-spike correction). There is no
//! app-level ping counter and no `ping()` in russh.

use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle};
use tokio::time::timeout;

use crate::error::AppError;
use crate::state::models::ForwardConfig;
use crate::state::AppState;

/// Connect timeout (spec 03 §1).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Auth timeout (spec 03 §1).
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);

/// The russh client handle used across the app. Aliased so callers don't repeat
/// the generic `ClientHandler` everywhere.
pub type Session = Handle<ClientHandler>;

/// Client-side session handler.
///
/// Host-key policy: accept any server key (TOFU/no-verify), matching v1's
/// `dartssh2` behavior which did not verify host keys. Revisit if host-key
/// pinning is ever specced.
pub struct ClientHandler;

#[async_trait::async_trait]
impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Comprehensive algorithm-negotiation set.
///
/// russh 0.45's `Preferred::DEFAULT` omits algorithms real-world OpenSSH jump
/// hosts still require — most importantly the `ssh-rsa` (SHA-1) host-key
/// signature and `ecdsa-sha2-nistp384`, plus the older `diffie-hellman-group14`
/// KEX and CTR/CBC ciphers. Against such a server the transport-layer
/// negotiation fails BEFORE authentication with russh's `Error::UnknownAlgo`
/// ("Unknown algorithm") / `NoCommonKexAlgo`. We therefore offer everything
/// russh 0.45 can actually instantiate.
///
/// CRITICAL: every name below is verified to be present in russh 0.45's
/// `KEXES` / `CIPHERS` / `MACS` / key registries. Listing a name russh does NOT
/// have registered is exactly what raises `Error::UnknownAlgo` (negotiation
/// selects a name from our list, then the registry lookup returns `None`), so
/// this list must never include an unregistered algorithm.
///
/// The `ext-info-*` / `kex-strict-*` entries are negotiation markers (not real
/// KEX algorithms) that russh's own default carries; keeping them preserves
/// SSH-extension + strict-KEX support, which the RSA-SHA2 negotiation relies on.
const PREFERRED_KEX: &[russh::kex::Name] = &[
    russh::kex::CURVE25519,
    russh::kex::CURVE25519_PRE_RFC_8731,
    russh::kex::DH_G16_SHA512,
    russh::kex::DH_G14_SHA256,
    russh::kex::ECDH_SHA2_NISTP256,
    russh::kex::ECDH_SHA2_NISTP384,
    russh::kex::ECDH_SHA2_NISTP521,
    russh::kex::DH_G14_SHA1,
    russh::kex::EXTENSION_SUPPORT_AS_CLIENT,
    russh::kex::EXTENSION_SUPPORT_AS_SERVER,
    russh::kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT,
    russh::kex::EXTENSION_OPENSSH_STRICT_KEX_AS_SERVER,
];

/// Host & public-key signature algorithms. Adds `ecdsa-sha2-nistp384` and
/// `ssh-rsa` (SHA-1 RSA) on top of russh's default — the likely culprit for the
/// `dbjump.qiscus.io` RSA host key.
const PREFERRED_KEY: &[russh::keys::key::Name] = &[
    russh::keys::key::ED25519,
    russh::keys::key::ECDSA_SHA2_NISTP256,
    russh::keys::key::ECDSA_SHA2_NISTP384,
    russh::keys::key::ECDSA_SHA2_NISTP521,
    russh::keys::key::RSA_SHA2_512,
    russh::keys::key::RSA_SHA2_256,
    russh::keys::key::SSH_RSA,
];

/// Symmetric ciphers. GCM/ChaCha are AEAD; CTR/CBC pair with the MAC list.
const PREFERRED_CIPHER: &[russh::cipher::Name] = &[
    russh::cipher::CHACHA20_POLY1305,
    russh::cipher::AES_256_GCM,
    russh::cipher::AES_256_CTR,
    russh::cipher::AES_192_CTR,
    russh::cipher::AES_128_CTR,
    russh::cipher::AES_256_CBC,
    russh::cipher::AES_192_CBC,
    russh::cipher::AES_128_CBC,
];

/// MAC algorithms (ETM preferred). Unused with AEAD ciphers, required for CTR/CBC.
const PREFERRED_MAC: &[russh::mac::Name] = &[
    russh::mac::HMAC_SHA256_ETM,
    russh::mac::HMAC_SHA512_ETM,
    russh::mac::HMAC_SHA256,
    russh::mac::HMAC_SHA512,
    russh::mac::HMAC_SHA1_ETM,
    russh::mac::HMAC_SHA1,
];

/// The comprehensive `Preferred` set offered during KEX (see [`PREFERRED_KEX`]).
fn preferred_algorithms() -> russh::Preferred {
    russh::Preferred {
        kex: Cow::Borrowed(PREFERRED_KEX),
        key: Cow::Borrowed(PREFERRED_KEY),
        cipher: Cow::Borrowed(PREFERRED_CIPHER),
        mac: Cow::Borrowed(PREFERRED_MAC),
        // Keep russh's default compression (feature-gated zlib handling).
        compression: russh::Preferred::DEFAULT.compression,
    }
}

/// Build the russh `client::Config` for a forward, applying effective-keepalive
/// normalization (spec 03 §2): interval `0 → 10s`, max `0 → 3`, and the
/// comprehensive algorithm-negotiation set ([`preferred_algorithms`]).
pub fn build_config(cfg: &ForwardConfig) -> client::Config {
    client::Config {
        keepalive_interval: Some(Duration::from_secs(cfg.effective_keepalive_interval_sec())),
        keepalive_max: cfg.effective_keepalive_max(),
        preferred: preferred_algorithms(),
        ..Default::default()
    }
}

/// SSH-connect with a 15s timeout (spec 03 §1 step 2). Returns the session
/// handle; the caller authenticates next.
pub async fn connect(cfg: &ForwardConfig) -> Result<Session, AppError> {
    let config = Arc::new(build_config(cfg));
    let addr = (cfg.ssh_host.as_str(), cfg.ssh_port);
    match timeout(
        CONNECT_TIMEOUT,
        client::connect(config, addr, ClientHandler),
    )
    .await
    {
        Ok(Ok(session)) => Ok(session),
        Ok(Err(e)) => Err(AppError::Ssh(format!("connect failed: {e}"))),
        Err(_) => Err(AppError::Connection(
            "SSH connect timed out after 15s".into(),
        )),
    }
}

/// Authenticate the session — password OR identity file, mutually exclusive,
/// with **identity precedence** when both are somehow present (spec 03 §1 step
/// 3, spec 04 §1). Wrapped in a 30s timeout.
pub async fn authenticate(
    session: &mut Session,
    cfg: &ForwardConfig,
    state: &AppState,
) -> Result<(), AppError> {
    match timeout(AUTH_TIMEOUT, authenticate_inner(session, cfg, state)).await {
        Ok(res) => res,
        Err(_) => Err(AppError::Connection(
            "SSH authentication timed out after 30s".into(),
        )),
    }
}

async fn authenticate_inner(
    session: &mut Session,
    cfg: &ForwardConfig,
    state: &AppState,
) -> Result<(), AppError> {
    // Identity file takes precedence over a password when both are set.
    let identity = cfg.identity_file_path.as_deref().filter(|p| !p.is_empty());

    if let Some(path) = identity {
        // load_secret_key does synchronous file I/O — run it off the async path.
        let path = path.to_string();
        let key = tokio::task::spawn_blocking(move || russh::keys::load_secret_key(&path, None))
            .await
            .map_err(|e| AppError::Ssh(format!("key load task failed: {e}")))?
            .map_err(|e| AppError::Ssh(format!("failed to load identity file: {e}")))?;
        let accepted = session
            .authenticate_publickey(&cfg.ssh_username, Arc::new(key))
            .await
            .map_err(|e| AppError::Ssh(format!("publickey auth error: {e}")))?;
        if !accepted {
            return Err(AppError::Ssh("publickey authentication rejected".into()));
        }
    } else {
        // Keyring get does blocking OS calls; run it off the async runtime (F38).
        let pw = state
            .get_password_async(&cfg.id)
            .await
            .ok_or_else(|| AppError::Connection("password or identity file required".into()))?;
        let accepted = session
            .authenticate_password(&cfg.ssh_username, pw)
            .await
            .map_err(|e| AppError::Ssh(format!("password auth error: {e}")))?;
        if !accepted {
            return Err(AppError::Ssh("password authentication rejected".into()));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::ForwardConfig;

    fn cfg_with_keepalive(interval: u32, max: u32) -> ForwardConfig {
        ForwardConfig {
            id: "t".into(),
            name: "t".into(),
            ssh_host: "h".into(),
            ssh_port: 22,
            ssh_username: "u".into(),
            identity_file_path: None,
            has_stored_password: false,
            local_bind_address: "127.0.0.1".into(),
            local_port: 1,
            remote_host: "r".into(),
            remote_port: 1,
            keep_alive_interval_sec: interval,
            keep_alive_max_count: max,
            group_id: None,
            tags: vec![],
        }
    }

    #[test]
    fn keepalive_normalization_zero_maps_to_defaults() {
        // spec 03 §2: interval 0 → 10s, max 0 → 3.
        let c = build_config(&cfg_with_keepalive(0, 0));
        assert_eq!(c.keepalive_interval, Some(Duration::from_secs(10)));
        assert_eq!(c.keepalive_max, 3);
    }

    #[test]
    fn keepalive_normalization_nonzero_passes_through() {
        let c = build_config(&cfg_with_keepalive(45, 7));
        assert_eq!(c.keepalive_interval, Some(Duration::from_secs(45)));
        assert_eq!(c.keepalive_max, 7);
    }

    #[test]
    fn preferred_offers_broad_host_key_and_kex_set() {
        // Regression for "SSH error: connect failed: Unknown algorithm" against
        // an OpenSSH jump host with an RSA host key: russh's default omits
        // `ssh-rsa`, `ecdsa-sha2-nistp384`, and the DH-group14 KEX. Assert the
        // widened set is what build_config offers.
        let p = preferred_algorithms();
        assert!(p.key.contains(&russh::keys::key::SSH_RSA));
        assert!(p.key.contains(&russh::keys::key::RSA_SHA2_256));
        assert!(p.key.contains(&russh::keys::key::RSA_SHA2_512));
        assert!(p.key.contains(&russh::keys::key::ECDSA_SHA2_NISTP384));
        assert!(p.kex.contains(&russh::kex::DH_G14_SHA256));
        assert!(p.kex.contains(&russh::kex::DH_G14_SHA1));
        assert!(p.cipher.contains(&russh::cipher::AES_256_CTR));

        let c = build_config(&cfg_with_keepalive(0, 0));
        assert!(c.preferred.key.contains(&russh::keys::key::SSH_RSA));
    }
}
