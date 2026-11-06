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
use crate::state::models::{ForwardConfig, HostKeyMismatch, HostKeyPin, HostKeySource};
use crate::state::AppState;

/// Connect timeout (spec 03 §1).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Auth timeout (spec 03 §1).
const AUTH_TIMEOUT: Duration = Duration::from_secs(30);

/// The russh client handle used across the app. Aliased so callers don't repeat
/// the generic `ClientHandler` everywhere.
pub type Session = Handle<ClientHandler>;

/// Client-side session handler. Its only job is the host-key check
/// (spec 03 §23): see [`decide`] for the policy.
pub struct ClientHandler {
    verifier: HostKeyVerifier,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(self.verifier.check(server_public_key))
    }
}

/// Host-key verification for one connection attempt (spec 03 §23).
///
/// Policy, in order:
/// 1. A key pinned on the forward is authoritative: it must match exactly.
/// 2. Otherwise `~/.ssh/known_hosts` is consulted READ-ONLY (never written):
///    an entry for this host with a different key of the same type refuses.
/// 3. Otherwise the key is accepted, and the engine pins it once
///    authentication succeeds (trust on first use, like OpenSSH).
///
/// The verdict is left in a shared slot because russh only lets the handler
/// answer yes/no; `connect` turns a refusal into a [`ConnectError::HostKey`].
pub struct HostKeyVerifier {
    host: String,
    port: u16,
    expected: Option<HostKeyPin>,
    known_hosts: Option<std::path::PathBuf>,
    outcome: Arc<std::sync::Mutex<Option<HostKeyOutcome>>>,
}

/// What the host-key check concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyOutcome {
    /// Accepted; carries the presented key (for pinning on first use).
    Accepted(HostKeyPin),
    /// Refused: the server presented a different key.
    Refused(HostKeyMismatch),
}

/// The `known_hosts` lookup result fed into [`decide`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownHostsVerdict {
    /// An entry for this host holds exactly this key.
    Match,
    /// An entry for this host holds a different key of the same type.
    Changed,
    /// No usable entry (no file, no line for the host, other key types).
    Unknown,
}

/// The pin form of a public key: algorithm + SHA-256 fingerprint as OpenSSH
/// prints them. Pure.
pub fn key_pin(key: &russh::keys::PublicKey) -> HostKeyPin {
    HostKeyPin {
        algorithm: key.algorithm().as_str().to_string(),
        fingerprint: key.fingerprint(russh::keys::HashAlg::Sha256).to_string(),
        pinned_at: None,
    }
}

/// The host-key policy as a pure function (unit-tested). See [`HostKeyVerifier`].
pub fn decide(
    expected: Option<&HostKeyPin>,
    presented: HostKeyPin,
    known_hosts: KnownHostsVerdict,
) -> HostKeyOutcome {
    match expected {
        Some(pin) => {
            if pin.algorithm == presented.algorithm && pin.fingerprint == presented.fingerprint {
                HostKeyOutcome::Accepted(presented)
            } else {
                HostKeyOutcome::Refused(HostKeyMismatch {
                    expected: Some(pin.clone()),
                    presented,
                    source: HostKeySource::Pinned,
                })
            }
        }
        None => match known_hosts {
            KnownHostsVerdict::Changed => HostKeyOutcome::Refused(HostKeyMismatch {
                expected: None,
                presented,
                source: HostKeySource::KnownHosts,
            }),
            KnownHostsVerdict::Match | KnownHostsVerdict::Unknown => {
                HostKeyOutcome::Accepted(presented)
            }
        },
    }
}

/// `TUNNEL_PILOT_KNOWN_HOSTS` (empty = do not consult one), else
/// `~/.ssh/known_hosts`.
pub fn default_known_hosts_path() -> Option<std::path::PathBuf> {
    if let Some(v) = std::env::var_os("TUNNEL_PILOT_KNOWN_HOSTS") {
        return (!v.is_empty()).then(|| std::path::PathBuf::from(v));
    }
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(|h| std::path::PathBuf::from(h).join(".ssh").join("known_hosts"))
}

impl HostKeyVerifier {
    pub fn new(cfg: &ForwardConfig, known_hosts: Option<std::path::PathBuf>) -> Self {
        Self {
            host: cfg.ssh_host.clone(),
            port: cfg.ssh_port,
            expected: cfg.host_key.clone(),
            known_hosts,
            outcome: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// The shared verdict slot (read by `connect` after the handshake).
    pub fn outcome_slot(&self) -> Arc<std::sync::Mutex<Option<HostKeyOutcome>>> {
        self.outcome.clone()
    }

    fn known_hosts_verdict(&self, key: &russh::keys::PublicKey) -> KnownHostsVerdict {
        let Some(path) = &self.known_hosts else {
            return KnownHostsVerdict::Unknown;
        };
        // A tiny, local, read-only file; russh's parser handles hashed hosts
        // and the `[host]:port` form.
        match russh::keys::check_known_hosts_path(&self.host, self.port, key, path) {
            Ok(true) => KnownHostsVerdict::Match,
            Err(russh::keys::Error::KeyChanged { .. }) => KnownHostsVerdict::Changed,
            _ => KnownHostsVerdict::Unknown,
        }
    }

    /// Decide for the presented key; `true` lets the handshake continue.
    pub fn check(&self, key: &russh::keys::PublicKey) -> bool {
        let presented = key_pin(key);
        let known = if self.expected.is_none() {
            self.known_hosts_verdict(key)
        } else {
            KnownHostsVerdict::Unknown // a pin is authoritative
        };
        let outcome = decide(self.expected.as_ref(), presented, known);
        let accepted = matches!(outcome, HostKeyOutcome::Accepted(_));
        if let Ok(mut slot) = self.outcome.lock() {
            *slot = Some(outcome);
        }
        accepted
    }
}

/// Why `connect` failed.
#[derive(Debug)]
pub enum ConnectError {
    /// The host-key check refused the server (never retried automatically).
    HostKey(Box<HostKeyMismatch>),
    /// Anything else (network, KEX, timeout).
    Other(AppError),
}

/// The user-facing explanation of a refused host key. Names both fingerprints
/// so the user can compare them out of band. Pure.
pub fn describe_mismatch(host: &str, port: u16, m: &HostKeyMismatch) -> String {
    let presented = format!("{} {}", m.presented.algorithm, m.presented.fingerprint);
    match (&m.source, &m.expected) {
        (HostKeySource::Pinned, Some(pin)) => format!(
            "host key changed for {host}:{port}: the server now presents {presented}, but \
             {} {} is pinned. If the server was reinstalled, verify the new fingerprint with \
             its administrator and trust it; otherwise someone may be intercepting the \
             connection",
            pin.algorithm, pin.fingerprint
        ),
        _ => format!(
            "host key for {host}:{port} does not match your ~/.ssh/known_hosts: the server \
             presents {presented}. Verify it with the server's administrator; if the server \
             legitimately changed, trust the new key here (known_hosts is never modified)"
        ),
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

/// SSH-connect with a 15s timeout (spec 03 §1 step 2), verifying the server's
/// host key (§23). Returns the session handle and the key the server presented
/// (for trust-on-first-use pinning); the caller authenticates next.
pub async fn connect(cfg: &ForwardConfig) -> Result<(Session, HostKeyPin), ConnectError> {
    let config = Arc::new(build_config(cfg));
    let addr = (cfg.ssh_host.as_str(), cfg.ssh_port);
    let verifier = HostKeyVerifier::new(cfg, default_known_hosts_path());
    let outcome = verifier.outcome_slot();
    let result = timeout(
        CONNECT_TIMEOUT,
        client::connect(config, addr, ClientHandler { verifier }),
    )
    .await;
    let verdict = outcome.lock().ok().and_then(|g| g.clone());
    match (result, verdict) {
        (Ok(Ok(session)), Some(HostKeyOutcome::Accepted(presented))) => Ok((session, presented)),
        (Ok(Ok(_)), _) => Err(ConnectError::Other(AppError::Ssh(
            "connect succeeded without a host-key check".into(),
        ))),
        (_, Some(HostKeyOutcome::Refused(mismatch))) => {
            Err(ConnectError::HostKey(Box::new(mismatch)))
        }
        (Ok(Err(e)), _) => Err(ConnectError::Other(AppError::Ssh(format!(
            "connect failed: {e}"
        )))),
        (Err(_), _) => Err(ConnectError::Other(AppError::Connection(
            "SSH connect timed out after 15s".into(),
        ))),
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
        let key = tokio::task::spawn_blocking(move || {
            let text = read_identity_file(&path)?;
            russh::keys::decode_secret_key(&text, None)
                .map_err(|e| AppError::Ssh(format!("failed to load identity file: {e}")))
        })
        .await
        .map_err(|e| AppError::Ssh(format!("key load task failed: {e}")))??;
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

/// Largest identity file we will read. Private keys are a few KiB; the cap
/// stops a configured path such as `/dev/zero` from exhausting memory.
pub const MAX_IDENTITY_BYTES: u64 = 64 * 1024;

/// Read an identity file, refusing anything that is not a regular file of a
/// plausible size. The path is configurable from every UI (including the web
/// UI of the server product), so it must not be able to name a device, a FIFO
/// that blocks forever, or a huge file. Errors never include file contents.
pub fn read_identity_file(path: &str) -> Result<String, AppError> {
    use std::io::Read;

    let meta =
        std::fs::metadata(path).map_err(|e| AppError::Ssh(format!("identity file {path}: {e}")))?;
    if !meta.is_file() {
        return Err(AppError::Ssh(format!(
            "identity file {path} is not a regular file"
        )));
    }
    if meta.len() > MAX_IDENTITY_BYTES {
        return Err(AppError::Ssh(format!(
            "identity file {path} is larger than {} KiB — not a private key",
            MAX_IDENTITY_BYTES / 1024
        )));
    }
    let mut text = String::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_IDENTITY_BYTES + 1).read_to_string(&mut text))
        .map_err(|e| AppError::Ssh(format!("identity file {path}: {e}")))?;
    if text.len() as u64 > MAX_IDENTITY_BYTES {
        return Err(AppError::Ssh(format!(
            "identity file {path} grew while reading"
        )));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::models::ForwardConfig;

    fn test_cfg() -> ForwardConfig {
        cfg_with_keepalive(30, 5)
    }

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
            host_key: None,
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

    #[test]
    fn identity_file_must_be_a_small_regular_file() {
        let dir = tempfile::tempdir().expect("tmp");
        assert!(
            read_identity_file(dir.path().to_str().expect("utf8")).is_err(),
            "a directory"
        );
        let big = dir.path().join("big");
        std::fs::write(&big, vec![b'a'; (MAX_IDENTITY_BYTES + 1) as usize]).expect("write");
        assert!(
            read_identity_file(big.to_str().expect("utf8")).is_err(),
            "too large"
        );
        let small = dir.path().join("key");
        std::fs::write(&small, "not really a key").expect("write");
        assert_eq!(
            read_identity_file(small.to_str().expect("utf8")).expect("read"),
            "not really a key"
        );
        #[cfg(unix)]
        assert!(
            read_identity_file("/dev/zero").is_err(),
            "a device is refused"
        );
        let err = read_identity_file("/nonexistent/key").expect_err("missing");
        assert!(
            !err.to_string().contains("really"),
            "errors carry no contents"
        );
    }

    // --- host-key policy (spec 03 §23) ---

    fn pin(alg: &str, fp: &str) -> HostKeyPin {
        HostKeyPin {
            algorithm: alg.into(),
            fingerprint: fp.into(),
            pinned_at: None,
        }
    }

    #[test]
    fn a_pinned_key_must_match_exactly_and_overrides_known_hosts() {
        let pinned = pin("ssh-ed25519", "SHA256:aaa");
        assert!(matches!(
            decide(
                Some(&pinned),
                pin("ssh-ed25519", "SHA256:aaa"),
                KnownHostsVerdict::Changed
            ),
            HostKeyOutcome::Accepted(_)
        ));
        for other in [
            pin("ssh-ed25519", "SHA256:bbb"),
            pin("ecdsa-sha2-nistp256", "SHA256:aaa"),
        ] {
            match decide(Some(&pinned), other.clone(), KnownHostsVerdict::Match) {
                HostKeyOutcome::Refused(m) => {
                    assert_eq!(m.source, HostKeySource::Pinned);
                    assert_eq!(m.expected.as_ref(), Some(&pinned));
                    assert_eq!(m.presented, other);
                }
                accepted => panic!("{other:?} must be refused, got {accepted:?}"),
            }
        }
    }

    #[test]
    fn without_a_pin_known_hosts_decides_then_first_use_is_trusted() {
        let key = pin("ssh-ed25519", "SHA256:aaa");
        assert!(matches!(
            decide(None, key.clone(), KnownHostsVerdict::Unknown),
            HostKeyOutcome::Accepted(_)
        ));
        assert!(matches!(
            decide(None, key.clone(), KnownHostsVerdict::Match),
            HostKeyOutcome::Accepted(_)
        ));
        match decide(None, key, KnownHostsVerdict::Changed) {
            HostKeyOutcome::Refused(m) => {
                assert_eq!(m.source, HostKeySource::KnownHosts);
                assert!(m.expected.is_none());
            }
            accepted => panic!("a known_hosts conflict must refuse, got {accepted:?}"),
        }
    }

    #[test]
    fn known_hosts_file_is_consulted_read_only() {
        use russh::keys::ssh_key::private::Ed25519Keypair;
        let ours = russh::keys::PrivateKey::from(Ed25519Keypair::from_seed(&[1u8; 32]));
        let other = russh::keys::PrivateKey::from(Ed25519Keypair::from_seed(&[2u8; 32]));
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("known_hosts");
        let line = format!(
            "[bastion.example.com]:2222 {}\n",
            ours.public_key().to_openssh().expect("openssh")
        );
        std::fs::write(&path, &line).expect("write");
        let mut cfg = test_cfg();
        cfg.ssh_host = "bastion.example.com".into();
        cfg.ssh_port = 2222;

        let v = HostKeyVerifier::new(&cfg, Some(path.clone()));
        assert!(v.check(ours.public_key()), "the recorded key is accepted");
        assert!(!v.check(other.public_key()), "a different key is refused");
        assert!(matches!(
            v.outcome_slot().lock().expect("slot").clone(),
            Some(HostKeyOutcome::Refused(HostKeyMismatch {
                source: HostKeySource::KnownHosts,
                ..
            }))
        ));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            line,
            "never written"
        );

        // Another host is unknown → first use is trusted.
        cfg.ssh_host = "elsewhere.example.com".into();
        assert!(HostKeyVerifier::new(&cfg, Some(path)).check(other.public_key()));
    }

    #[test]
    fn key_pin_uses_openssh_names_and_sha256() {
        use russh::keys::ssh_key::private::Ed25519Keypair;
        let key = russh::keys::PrivateKey::from(Ed25519Keypair::from_seed(&[3u8; 32]));
        let p = key_pin(key.public_key());
        assert_eq!(p.algorithm, "ssh-ed25519");
        assert!(p.fingerprint.starts_with("SHA256:"), "{}", p.fingerprint);
    }

    #[test]
    fn mismatch_message_names_both_fingerprints() {
        let m = HostKeyMismatch {
            expected: Some(pin("ssh-ed25519", "SHA256:old")),
            presented: pin("ssh-ed25519", "SHA256:new"),
            source: HostKeySource::Pinned,
        };
        let msg = describe_mismatch("bastion", 22, &m);
        assert!(msg.contains("SHA256:old") && msg.contains("SHA256:new"));
        assert!(msg.contains("intercepting"));
    }
}
