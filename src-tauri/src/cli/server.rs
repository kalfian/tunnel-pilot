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
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
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

/// Prepare the path (dir 0700 when we create it, stale socket cleared), bind,
/// and chmod 0600.
pub async fn bind(path: &Path) -> Result<UnixListener, AppError> {
    crate::cli::check_socket_path_len(path)?;
    if let Some(dir) = path.parent() {
        ensure_socket_dir(dir).await?;
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

/// Create the socket's parent directory owner-only — but only when WE create
/// it.
///
/// An existing directory keeps its own mode: `TUNNEL_PILOT_SOCKET` may point at
/// `$TMPDIR` or a user directory, and chmod'ing those is either impossible
/// (`/tmp` is root-owned `1777` ⇒ EPERM ⇒ the documented escape hatch never
/// binds) or rude (silently re-moding a directory the user chose). The socket
/// itself is chmod'd 0600 right after bind, which is the access control that
/// matters; the 0700 dir is defense in depth for the path we own.
async fn ensure_socket_dir(dir: &Path) -> Result<(), AppError> {
    if tokio::fs::symlink_metadata(dir).await.is_ok() {
        return Ok(()); // pre-existing — not ours to re-mode
    }
    // `recursive` applies DIR_MODE to every component this creates, and is a
    // no-op (not an error) if a concurrent launch won the race.
    tokio::fs::DirBuilder::new()
        .recursive(true)
        .mode(DIR_MODE)
        .create(dir)
        .await?;
    Ok(())
}

/// A leftover socket file from a crashed instance would make `bind` fail with
/// `EADDRINUSE`. Probe it: if something answers, another instance is live (the
/// single-instance plugin should have prevented this) and we must NOT steal the
/// path; if nothing answers, the file is stale and gets unlinked.
async fn clear_stale_socket(path: &Path) -> Result<(), AppError> {
    let meta = match tokio::fs::symlink_metadata(path).await {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    // Only ever unlink an actual socket. `TUNNEL_PILOT_SOCKET` is user input:
    // a typo pointing at a real file must fail loudly, not delete it.
    if !meta.file_type().is_socket() {
        return Err(AppError::InvalidInput(format!(
            "{} exists and is not a socket; refusing to remove it. Point {} somewhere else",
            path.display(),
            crate::cli::SOCKET_ENV
        )));
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
/// dead socket. A missing file is not an error — and a path that is NOT a
/// socket is left alone (same reasoning as [`clear_stale_socket`]: the path can
/// come from a mistyped `TUNNEL_PILOT_SOCKET`).
pub fn remove_socket(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_socket() => {
            tracing::warn!(
                socket = %path.display(),
                "control socket path is not a socket; leaving it untouched"
            );
            return;
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::warn!(socket = %path.display(), error = %e, "cannot stat the control socket");
            return;
        }
    }
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
