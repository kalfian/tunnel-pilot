//! russh client `Handler`, session config, connect, and authentication
//! (spec 03 §§1,2).
//!
//! Liveness is owned by russh keepalive: [`build_config`] sets
//! `keepalive_interval`/`keepalive_max`, so when the peer misses
//! `keepalive_max` keepalives russh's session task exits — observed by the
//! supervisor via `Handle::is_closed()` (F7, F16-spike correction). There is no
//! app-level ping counter and no `ping()` in russh.

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

/// Build the russh `client::Config` for a forward, applying effective-keepalive
/// normalization (spec 03 §2): interval `0 → 10s`, max `0 → 3`.
pub fn build_config(cfg: &ForwardConfig) -> client::Config {
    client::Config {
        keepalive_interval: Some(Duration::from_secs(cfg.effective_keepalive_interval_sec())),
        keepalive_max: cfg.effective_keepalive_max(),
        ..Default::default()
    }
}

/// SSH-connect with a 15s timeout (spec 03 §1 step 2). Returns the session
/// handle; the caller authenticates next.
pub async fn connect(cfg: &ForwardConfig) -> Result<Session, AppError> {
    let config = Arc::new(build_config(cfg));
    let addr = (cfg.ssh_host.as_str(), cfg.ssh_port);
    match timeout(CONNECT_TIMEOUT, client::connect(config, addr, ClientHandler)).await {
        Ok(Ok(session)) => Ok(session),
        Ok(Err(e)) => Err(AppError::Ssh(format!("connect failed: {e}"))),
        Err(_) => Err(AppError::Connection("SSH connect timed out after 15s".into())),
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
        Err(_) => Err(AppError::Connection("SSH authentication timed out after 30s".into())),
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
        let pw = state
            .get_password(&cfg.id)
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
}
