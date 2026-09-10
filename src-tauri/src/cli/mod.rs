//! Local control socket + CLI client (spec 02 §9, 03 §20).
//!
//! A second *driver* of the existing service functions — never a second state
//! owner. The socket handler resolves a target, calls the same
//! `ssh::engine::*` / `commands::forwards::run_*` entry points the tray and the
//! webview use, and serializes the result back as one JSON line. Status writes,
//! events, and persistence all keep flowing through `AppState` exactly as
//! before, so a CLI-driven connect updates the tray and (when open) the window.
//!
//! Transport is a Unix domain socket under the app config dir; the listener is
//! `#[cfg(unix)]` (a Windows named pipe would slot in behind the same `serve`
//! shape). The socket carries **no secrets**: there is no password command and
//! `ForwardView` exposes only the `hasStoredPassword` flag (AGENTS §8).

pub mod args;
pub mod protocol;
pub mod service;

use std::path::{Path, PathBuf};

use crate::error::AppError;

/// Bundle identifier — MUST match `tauri.conf.json`'s `identifier`, because the
/// CLI resolves the socket path *without* a Tauri context (there is no
/// `AppHandle` in the client process). A drift test pins the two together.
pub const IDENTIFIER: &str = "com.kalfian.tunnelpilot";

/// Sub-directory of the app config dir holding the control socket.
pub const SOCKET_DIR: &str = "cli";

/// Socket filename inside [`SOCKET_DIR`].
pub const SOCKET_FILE: &str = "cli.sock";

/// Environment override honored by BOTH sides (server bind + client connect).
/// The escape hatch when the default path exceeds the `sun_path` limit.
pub const SOCKET_ENV: &str = "TUNNEL_PILOT_SOCKET";

/// `sockaddr_un.sun_path` is 104 bytes on macOS / 108 on Linux, NUL included.
/// We refuse anything over 100 bytes so the failure is an explicit, actionable
/// log line instead of an opaque `EINVAL` from `bind(2)`.
pub const MAX_SOCKET_PATH_BYTES: usize = 100;

/// The control socket inside a given app config dir (no env override).
pub fn socket_path_in(config_dir: &Path) -> PathBuf {
    config_dir.join(SOCKET_DIR).join(SOCKET_FILE)
}

/// Server-side socket path: [`SOCKET_ENV`] when set and non-empty, else
/// [`socket_path_in`] of the app config dir Tauri resolved.
pub fn resolved_socket_path(config_dir: &Path) -> PathBuf {
    match socket_path_from_env() {
        Some(p) => p,
        None => socket_path_in(config_dir),
    }
}

/// The [`SOCKET_ENV`] override, if set to a non-empty value.
pub fn socket_path_from_env() -> Option<PathBuf> {
    std::env::var_os(SOCKET_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Client-side socket path: [`SOCKET_ENV`] first, else the platform app-config
/// dir + [`IDENTIFIER`] — mirroring Tauri's `app_config_dir()` resolution,
/// which the client process cannot call (it has no Tauri context).
pub fn default_socket_path() -> Option<PathBuf> {
    if let Some(p) = socket_path_from_env() {
        return Some(p);
    }
    platform_config_dir().map(|d| socket_path_in(&d.join(IDENTIFIER)))
}

/// `dirs::config_dir()` equivalent for the platforms we ship a CLI on. macOS:
/// `$HOME/Library/Application Support`; Linux: `$XDG_CONFIG_HOME` or
/// `$HOME/.config`. Kept dependency-free — this is the only place the client
/// needs to agree with Tauri.
fn platform_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join("Library").join("Application Support"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()) {
            return Some(PathBuf::from(xdg));
        }
        std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(".config"))
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Reject a socket path that would overflow `sun_path` (see
/// [`MAX_SOCKET_PATH_BYTES`]). The message names [`SOCKET_ENV`] so the fix is
/// obvious from the log line alone.
pub fn check_socket_path_len(path: &Path) -> Result<(), AppError> {
    let len = path.as_os_str().as_encoded_bytes().len();
    if len > MAX_SOCKET_PATH_BYTES {
        return Err(AppError::InvalidInput(format!(
            "control socket path is {len} bytes, over the {MAX_SOCKET_PATH_BYTES}-byte \
             sun_path limit: {}. Set {SOCKET_ENV} to a shorter path (e.g. /tmp/tp.sock).",
            path.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The identifier is duplicated (Rust const vs `tauri.conf.json`) because
    /// the client has no Tauri context. Pin them together so a rename of one
    /// cannot silently point the CLI at a socket the app never binds.
    #[test]
    fn identifier_matches_tauri_conf() {
        let conf = include_str!("../../tauri.conf.json");
        let parsed: serde_json::Value =
            serde_json::from_str(conf).expect("tauri.conf.json must be valid JSON");
        let identifier = parsed
            .get("identifier")
            .and_then(|v| v.as_str())
            .expect("tauri.conf.json must carry an identifier");
        assert_eq!(
            identifier, IDENTIFIER,
            "cli::IDENTIFIER drifted from tauri.conf.json — the CLI would look for the \
             socket in the wrong directory"
        );
    }

    #[test]
    fn socket_path_is_config_dir_cli_cli_sock() {
        let p = socket_path_in(Path::new("/tmp/cfg"));
        assert_eq!(p, PathBuf::from("/tmp/cfg/cli/cli.sock"));
    }

    #[test]
    fn path_length_guard_accepts_a_typical_path() {
        // ~75 bytes: the realistic macOS default for a normal $HOME.
        let p = PathBuf::from(
            "/Users/someone/Library/Application Support/com.kalfian.tunnelpilot/cli/cli.sock",
        );
        assert!(p.as_os_str().as_encoded_bytes().len() <= MAX_SOCKET_PATH_BYTES);
        assert!(check_socket_path_len(&p).is_ok());
    }

    #[test]
    fn path_length_guard_rejects_and_names_the_env_var() {
        let long = PathBuf::from(format!("/tmp/{}/cli.sock", "x".repeat(120)));
        let err = check_socket_path_len(&long).expect_err("must reject an over-long path");
        let msg = err.to_string();
        assert!(msg.contains(SOCKET_ENV), "error must name the env override");
        assert!(msg.contains("sun_path"), "error must explain the limit");
    }
}
