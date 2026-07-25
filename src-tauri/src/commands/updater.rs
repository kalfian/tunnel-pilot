//! Updater commands (spec 02 §6.6): `check_update`, `install_update`,
//! `skip_update`.
//!
//! Thin IPC wrappers over [`crate::updater`] (spec 03 §16). The real self-update
//! flow — minisign-signed bundle download + verify + install with
//! `update://progress`, and `update://status` on availability — lives in the
//! updater module; these commands just marshal Tauri state into it. The
//! JS-facing argument contract is unchanged (`check`/`install` take no args,
//! `skip` takes `{ version }`); the `AppHandle`/`State` params are auto-injected
//! by Tauri and never appear on the wire.

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::error::AppError;
use crate::state::models::UpdateStatus;
use crate::state::AppState;
use crate::updater::UpdaterState;

/// `check_update` — query the updater endpoint, honor `lastSkippedVersion`, cache
/// the pending update for [`install_update`], and emit `update://status`. This
/// is the user-triggered check, so it does NOT fire a notification (the auto
/// startup check owns the once-per-version notice).
#[tauri::command]
pub async fn check_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    updater: State<'_, Arc<UpdaterState>>,
) -> Result<UpdateStatus, AppError> {
    crate::updater::run_check(&app, state.inner(), updater.inner(), false).await
}

/// `install_update` — download + verify the minisign signature + install the
/// pending update (from the last `check_update`), emitting `update://progress`,
/// then relaunch. Errors if there is no pending update or the bundle fails
/// signature verification.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    updater: State<'_, Arc<UpdaterState>>,
) -> Result<(), AppError> {
    crate::updater::run_install(&app, updater.inner()).await
}

/// `skip_update` — remember that the user dismissed `version` so it is not
/// offered again (spec 02 §6.6). Persists `lastSkippedVersion`, notifies the
/// frontend, and (F51) refreshes the cached update status + re-emits
/// `update://status` so the tray notice — which reads the cached `latest_status`
/// and gates on `available && !skipped` — hides the just-skipped version
/// immediately, not only after the next `check_update`/restart.
#[tauri::command]
pub async fn skip_update(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    updater: State<'_, Arc<UpdaterState>>,
    version: String,
) -> Result<(), AppError> {
    let state = state.inner();
    let mut settings = state.settings_snapshot();
    settings.last_skipped_version = Some(version.clone());
    state.set_settings(settings);
    state.persist_settings().await?;
    state.emit_settings_changed();
    crate::updater::apply_skip(&app, updater.inner(), &version);
    Ok(())
}
