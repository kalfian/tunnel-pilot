//! Library-facing error type.
//!
//! Every `#[tauri::command]` returns `Result<T, AppError>` (AGENTS.md §4).
//! `AppError` uses `thiserror` for `Display`/`std::error::Error` and derives
//! `Serialize` so it crosses the Tauri IPC boundary as a structured value the
//! frontend can pattern-match on: `{ "kind": "<variant>", "message": "<text>" }`.
//!
//! `anyhow` is used only at binary edges (setup/main), never here.

use serde::Serialize;

/// The single error type surfaced across IPC.
///
/// Serialized as an internally-tagged object, e.g. a `Ssh` variant becomes
/// `{ "kind": "ssh", "message": "connection refused" }`.
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message", rename_all = "camelCase")]
pub enum AppError {
    /// SSH transport / protocol failure (russh).
    #[error("SSH error: {0}")]
    Ssh(String),

    /// Connection lifecycle failure (bind, connect, auth, forward).
    #[error("connection error: {0}")]
    Connection(String),

    /// A referenced entity (forward, group) does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// User-supplied input failed validation.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Configuration read/parse/merge failure.
    #[error("configuration error: {0}")]
    Config(String),

    /// Persistence (atomic read/write) failure.
    #[error("storage error: {0}")]
    Storage(String),

    /// Backup export/import failure (incl. version rejection).
    #[error("backup error: {0}")]
    Backup(String),

    /// Keychain / credential store failure.
    #[error("credential error: {0}")]
    Credential(String),

    /// Self-updater failure (check/download/verify/install).
    #[error("updater error: {0}")]
    Updater(String),

    /// (De)serialization failure.
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Underlying I/O failure.
    #[error("I/O error: {0}")]
    Io(String),

    /// Catch-all for unexpected internal failures.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Convenience alias for library code: `Result<T> = std::result::Result<T, AppError>`.
pub type Result<T> = std::result::Result<T, AppError>;

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Serialization(e.to_string())
    }
}
