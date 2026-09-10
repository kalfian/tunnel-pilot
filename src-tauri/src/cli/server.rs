//! The Unix-domain control socket listener (spec 03 §20).
//!
//! Lifecycle: create `<appConfigDir>/cli/` with mode 0700 → probe/clear a stale
//! socket → bind → chmod the socket 0600 → accept forever. Every failure is
//! logged and swallowed by [`serve`]: the CLI is a convenience, and must never
//! stop the app from starting.
//!
//! Security is filesystem permissions only, and deliberately so: the socket
//! exposes capabilities the same local user already holds (they own the GUI,
//! the config file and the keychain entries). A shared token would live in a
//! file with identical permissions — no added protection, more failure modes.
//! Root is out of the threat model (root wins regardless) and a UDS is not
//! network-reachable.

use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};

use crate::cli::protocol::{CliRequest, CliResponse};
use crate::cli::service;
use crate::error::AppError;
use crate::state::AppState;

/// Only the owner may traverse the socket directory.
const DIR_MODE: u32 = 0o700;
/// Only the owner may connect (defense in depth — some platforms historically
/// ignored permission bits on sockets, which is what [`DIR_MODE`] closes).
const SOCKET_MODE: u32 = 0o600;
/// Hard cap on what one connection may send us, so a rogue client cannot make
/// the app allocate without bound. Requests are a few hundred bytes.
const MAX_REQUEST_BYTES: u64 = 64 * 1024;
/// A connection that sends nothing for this long is dropped (the CLI writes its
/// request immediately). Applies to reads only — a long `connect --timeout 300`
/// is spent *processing*, not reading.
const IDLE_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// Backoff after an `accept()` error so a persistently failing listener cannot
/// spin the runtime.
const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(200);

/// Bind the control socket and serve it forever. Never panics and never returns
/// an error to the caller — a listener failure is logged and the app carries on
/// without a CLI.
pub async fn serve(state: Arc<AppState>, path: PathBuf) {
    match bind(&path).await {
        Ok(listener) => {
            tracing::info!(socket = %path.display(), "CLI control socket listening");
            serve_on(state, listener).await;
        }
        Err(e) => {
            tracing::error!(
                socket = %path.display(),
                error = %e,
                "CLI control socket unavailable; the app runs normally without it"
            );
        }
    }
}

/// Prepare the path (dir 0700, stale socket cleared), bind, and chmod 0600.
pub async fn bind(path: &Path) -> Result<UnixListener, AppError> {
    crate::cli::check_socket_path_len(path)?;
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
        tokio::fs::set_permissions(dir, Permissions::from_mode(DIR_MODE)).await?;
    }
    clear_stale_socket(path).await?;
    let listener = UnixListener::bind(path)?;
    tokio::fs::set_permissions(path, Permissions::from_mode(SOCKET_MODE)).await?;
    Ok(listener)
}

/// Accept loop. One task per connection; a per-connection failure is logged at
/// debug level (a client hanging up mid-response is normal) and never kills the
/// listener.
pub async fn serve_on(state: Arc<AppState>, listener: UnixListener) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(state, stream).await {
                        tracing::debug!(error = %e, "CLI connection ended early");
                    }
                });
            }
            Err(e) => {
                tracing::error!(error = %e, "CLI control socket accept failed");
                tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
            }
        }
    }
}

/// A leftover socket file from a crashed instance would make `bind` fail with
/// `EADDRINUSE`. Probe it: if something answers, another instance is live (the
/// single-instance plugin should have prevented this) and we must NOT steal the
/// path; if nothing answers, the file is stale and gets unlinked.
async fn clear_stale_socket(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }
    match UnixStream::connect(path).await {
        Ok(_) => Err(AppError::Internal(format!(
            "another Tunnel Pilot instance is already serving {}",
            path.display()
        ))),
        Err(_) => {
            tracing::warn!(socket = %path.display(), "removing stale control socket");
            tokio::fs::remove_file(path).await?;
            Ok(())
        }
    }
}

/// Best-effort unlink, used on quit so a fresh launch does not have to probe a
/// dead socket. A missing file is not an error.
pub fn remove_socket(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => tracing::debug!(socket = %path.display(), "control socket removed"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            tracing::warn!(socket = %path.display(), error = %e, "failed to remove control socket")
        }
    }
}

/// Read NDJSON request lines until EOF, answering each with one response line.
async fn handle_connection(state: Arc<AppState>, stream: UnixStream) -> Result<(), AppError> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half.take(MAX_REQUEST_BYTES));
    let mut line = String::new();

    loop {
        line.clear();
        let read = match tokio::time::timeout(IDLE_READ_TIMEOUT, reader.read_line(&mut line)).await
        {
            Ok(result) => result?,
            Err(_) => {
                tracing::debug!("CLI connection idle; closing");
                return Ok(());
            }
        };
        if read == 0 {
            return Ok(()); // EOF — the client is done.
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = match serde_json::from_str::<CliRequest>(trimmed) {
            Ok(req) => {
                tracing::debug!(cmd = ?req.cmd, target = ?req.target, "CLI request");
                service::handle_request(&state, req).await
            }
            Err(e) => CliResponse::err(AppError::InvalidInput(format!(
                "malformed request line: {e}"
            ))),
        };

        let mut payload = serde_json::to_vec(&response)?;
        payload.push(b'\n');
        write_half.write_all(&payload).await?;
        write_half.flush().await?;
    }
}
