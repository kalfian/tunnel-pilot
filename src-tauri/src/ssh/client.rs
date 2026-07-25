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

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Host & public-key signature algorithms offered to the server.
///
/// Covers Ed25519, the three NIST ECDSA curves, RSA with SHA-2 (`rsa-sha2-512`
/// / `rsa-sha2-256`), and legacy `ssh-rsa` (SHA-1) — the last kept for older
/// jump hosts whose RSA host key only signs with SHA-1. russh 0.62's default
/// already includes all of these; we list them explicitly so the offered set is
/// pinned and unit-testable.
///
/// In russh 0.62 host-key preferences are `ssh_key::Algorithm` values (not the
/// name-string constants that russh 0.45 used).
const PREFERRED_KEY: &[russh::keys::Algorithm] = &[
    russh::keys::Algorithm::Ed25519,
    russh::keys::Algorithm::Ecdsa {
        curve: russh::keys::EcdsaCurve::NistP256,
    },
    russh::keys::Algorithm::Ecdsa {
        curve: russh::keys::EcdsaCurve::NistP384,
    },
    russh::keys::Algorithm::Ecdsa {
        curve: russh::keys::EcdsaCurve::NistP521,
    },
    russh::keys::Algorithm::Rsa {
        hash: Some(russh::keys::HashAlg::Sha512),
    },
    russh::keys::Algorithm::Rsa {
        hash: Some(russh::keys::HashAlg::Sha256),
    },
    // `Rsa { hash: None }` == the legacy `ssh-rsa` (SHA-1) signature.
    russh::keys::Algorithm::Rsa { hash: None },
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

/// The `Preferred` algorithm set offered during KEX.
///
/// `kex` and `compression` are inherited verbatim from `Preferred::DEFAULT`.
/// This is deliberate and load-bearing: russh 0.62 advertises the ext-info /
/// strict-kex signaling MARKERS only if they are present in `prefs.kex`, and it
/// places/orders them there itself. Hand-rolling a bespoke kex list (as the
/// russh-0.45 code did) would silently drop those markers and disable strict-kex
/// signaling. russh 0.62 then also correctly EXCLUDES the markers from real KEX
/// selection (negotiation.rs filters `KEX_EXTENSION_NAMES` before choosing),
/// which is exactly the fix for the 0.45 bug where the marker could be selected
/// as the KEX and blow up with `Error::UnknownAlgo`. So: let the library own the
/// kex list; we only broaden host keys / ciphers / MACs for jump-host reach.
fn preferred_algorithms() -> russh::Preferred {
    russh::Preferred {
        kex: russh::Preferred::DEFAULT.kex,
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
        // In russh 0.62 it returns an `ssh_key::PrivateKey`.
        let path = path.to_string();
        let key = tokio::task::spawn_blocking(move || russh::keys::load_secret_key(&path, None))
            .await
            .map_err(|e| AppError::Ssh(format!("key load task failed: {e}")))?
            .map_err(|e| AppError::Ssh(format!("failed to load identity file: {e}")))?;
        let key = Arc::new(key);

        // For RSA keys, pick the strongest signature hash the server advertised
        // via `server-sig-algs` (rsa-sha2-512 > rsa-sha2-256 > legacy ssh-rsa).
        // Modern OpenSSH (8.x+) rejects SHA-1 `ssh-rsa` for user auth, so passing
        // `None` (which maps to ssh-rsa) would fail against `dbjump.qiscus.io`.
        // Non-RSA keys ignore the hash, so only probe when the key is RSA.
        let hash_alg = if key.algorithm().is_rsa() {
            session
                .best_supported_rsa_hash()
                .await
                .ok()
                .flatten()
                .flatten()
        } else {
            None
        };

        let key = russh::keys::PrivateKeyWithHashAlg::new(key, hash_alg);
        let result = session
            .authenticate_publickey(&cfg.ssh_username, key)
            .await
            .map_err(|e| AppError::Ssh(format!("publickey auth error: {e}")))?;
        if !result.success() {
            return Err(AppError::Ssh("publickey authentication rejected".into()));
        }
    } else {
        // Keyring get does blocking OS calls; run it off the async runtime (F38).
        let pw = state
            .get_password_async(&cfg.id)
            .await
            .ok_or_else(|| AppError::Connection("password or identity file required".into()))?;
        let result = session
            .authenticate_password(&cfg.ssh_username, pw)
            .await
            .map_err(|e| AppError::Ssh(format!("password auth error: {e}")))?;
        if !result.success() {
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
        // Regression for the strict-kex negotiation failure against OpenSSH 8.9
        // jump hosts (`dbjump.qiscus.io`). Two invariants:
        //   1. Host keys include legacy `ssh-rsa` (SHA-1) + both RSA SHA-2
        //      variants + ecdsa-nistp384, so an RSA-host-key jump host matches.
        //   2. The kex list carries curve25519 AND the ext-info / strict-kex
        //      signaling markers (inherited from `Preferred::DEFAULT`) — russh
        //      0.62 needs the markers present to advertise strict-kex, then
        //      excludes them from real KEX selection (the 0.45 bug fix).
        let p = preferred_algorithms();
        assert!(p.key.contains(&russh::keys::Algorithm::Rsa { hash: None }));
        assert!(p.key.contains(&russh::keys::Algorithm::Rsa {
            hash: Some(russh::keys::HashAlg::Sha256)
        }));
        assert!(p.key.contains(&russh::keys::Algorithm::Rsa {
            hash: Some(russh::keys::HashAlg::Sha512)
        }));
        assert!(p.key.contains(&russh::keys::Algorithm::Ecdsa {
            curve: russh::keys::EcdsaCurve::NistP384
        }));
        assert!(p.kex.contains(&russh::kex::CURVE25519));
        assert!(p
            .kex
            .contains(&russh::kex::EXTENSION_OPENSSH_STRICT_KEX_AS_CLIENT));
        assert!(p.cipher.contains(&russh::cipher::AES_256_CTR));

        let c = build_config(&cfg_with_keepalive(0, 0));
        assert!(c
            .preferred
            .key
            .contains(&russh::keys::Algorithm::Rsa { hash: None }));
    }
}
