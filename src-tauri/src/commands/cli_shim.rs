//! CLI-on-PATH commands (spec 02 §6.8): `cli_shim_status`,
//! `install_cli_shim`, `uninstall_cli_shim`.
//!
//! Thin handlers over [`crate::cli::shim`] — all the rules (candidate choice,
//! elevation, dev-build refusal, never-delete-a-real-file) live there so the
//! CLI subcommands and the Settings UI behave identically.
//!
//! `install`/`uninstall` are `async` and hop to the blocking pool: they touch
//! the filesystem and, when the target needs root, run `osascript` and wait for
//! the user to type a password — neither belongs on the main thread.

use crate::cli::shim::{self, CliShimStatus, ShimTarget};
use crate::error::AppError;

/// `cli_shim_status` — where the shim is, whether it points at THIS binary, and
/// where an install would go (including whether that needs an admin prompt).
#[tauri::command]
pub fn cli_shim_status() -> CliShimStatus {
    shim::shim_status()
}

/// `install_cli_shim` — symlink the running binary into a bin directory on
/// `$PATH`. `target` forces a location; `None` picks the first writable one
/// (`~/.local/bin`, then `/usr/local/bin` with elevation).
#[tauri::command]
pub async fn install_cli_shim(target: Option<ShimTarget>) -> Result<CliShimStatus, AppError> {
    run_blocking(move || shim::install(target)).await
}

/// `uninstall_cli_shim` — remove our symlink from every candidate directory.
/// Idempotent, and never touches a path that is not a symlink of ours.
#[tauri::command]
pub async fn uninstall_cli_shim() -> Result<CliShimStatus, AppError> {
    run_blocking(shim::uninstall).await
}

/// Run a blocking shim operation off the async runtime, mapping a join failure
/// (panic in the closure) onto the app error vocabulary.
async fn run_blocking<F>(op: F) -> Result<CliShimStatus, AppError>
where
    F: FnOnce() -> Result<CliShimStatus, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(op)
        .await
        .map_err(|e| AppError::Internal(format!("CLI shim task failed: {e}")))?
}
