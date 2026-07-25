//! Self-updater (`tauri-plugin-updater`, minisign-signed bundles) — spec 03 §16.
//!
//! Two distinct signing concepts (do NOT conflate):
//! 1. **Updater bundle signing (minisign) — enforced from day one.** Each update
//!    bundle is signed in CI with the minisign private key and verified here
//!    against the public key embedded in `tauri.conf.json`
//!    (`plugins.updater.pubkey`). `download_and_install` performs that
//!    verification internally; a tampered/unsigned bundle makes it return `Err`,
//!    which we surface as [`AppError::Updater`]. We never weaken this (AGENTS §8).
//! 2. **OS code-signing / notarization — deferred post-v2.0.** Independent of #1;
//!    v2.0 ships unsigned at the OS level (spec 06 §4).
//!
//! Flow:
//! - [`run_check`] queries the GitHub Releases `latest.json` endpoint, compares
//!   versions, honors `lastSkippedVersion`, caches the pending [`Update`] for
//!   install, emits `update://status`, and (on the auto path) fires the
//!   update-available notification **once per version**.
//! - [`run_install`] downloads → verifies the minisign signature → installs,
//!   emitting `update://progress` chunks, then relaunches.
//! - [`auto_check_on_startup`] runs one check at boot iff `autoCheckUpdates`,
//!   swallowing errors so a failed check never disrupts startup.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::error::AppError;
use crate::events;
use crate::state::models::UpdateStatus;
use crate::state::AppState;

/// Payload for `update://progress` (spec 02 §7, events.rs):
/// `{ downloaded, total }` bytes. `total` is `None` when the server omits a
/// content-length.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// Updater runtime state, managed as `Arc<UpdaterState>` alongside [`AppState`].
///
/// Holds the pending [`Update`] returned by the last successful [`run_check`] so
/// [`run_install`] can act on it without a second network round-trip, plus the
/// last version we fired an update-available notification for (once-per-version
/// dedup, spec 03 §15). The dedup is in-memory/per-session: `lastSkippedVersion`
/// (persisted) already prevents re-offering a version the user dismissed, so an
/// at-most-once-per-restart notice for a still-pending update is acceptable and
/// avoids an FE-coupled `AppSettings` schema change (see report / AGENTS §9).
#[derive(Default)]
pub struct UpdaterState {
    /// The pending update from the last `check` (if any) — consumed by install.
    pending: tokio::sync::Mutex<Option<Update>>,
    /// Version we last notified about (once-per-version notification guard).
    last_notified_version: std::sync::Mutex<Option<String>>,
    /// Latest known availability snapshot. Cached so a late `app_hydrate` (window
    /// shown after the boot check already emitted `update://status`) still sees
    /// the current state instead of a stale not-available default.
    latest_status: std::sync::Mutex<UpdateStatus>,
}

impl UpdaterState {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_notified(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.last_notified_version
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The latest known update-availability snapshot (for `app_hydrate`).
    pub fn latest_status(&self) -> UpdateStatus {
        self.latest_status
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    fn set_latest_status(&self, status: UpdateStatus) {
        if let Ok(mut g) = self.latest_status.lock() {
            *g = status;
        }
    }

    /// Mark `version` as skipped in the cached latest status (F51). Only mutates
    /// when `version` is the version currently cached, so a stale skip of some
    /// other version can never flip the flag on the current offer. Returns the
    /// refreshed status for re-emitting `update://status`.
    fn mark_skipped(&self, version: &str) -> UpdateStatus {
        let mut g = self.latest_status.lock().unwrap_or_else(|e| e.into_inner());
        if g.version.as_deref() == Some(version) {
            g.skipped = true;
        }
        g.clone()
    }
}

/// Apply a user skip to the cached update status and re-emit `update://status`
/// (F51). Keeps the cache the single source of truth: the tray reads
/// `latest_status` + listens on `update://status` and gates the notice on
/// `available && !skipped`, so both the tray and the FE banner hide the just-
/// skipped version immediately — not only after the next `check`/restart.
pub fn apply_skip(app: &AppHandle, updater_state: &UpdaterState, version: &str) -> UpdateStatus {
    let status = updater_state.mark_skipped(version);
    emit_status(app, &status);
    status
}

/// Emit `update://status` (best-effort — a dropped emit is not fatal).
fn emit_status(app: &AppHandle, status: &UpdateStatus) {
    let _ = app.emit(events::UPDATE_STATUS, status);
}

/// Which trigger initiated an update check (BUG 3). The two paths differ ONLY in
/// how a check *failure* is surfaced:
///
/// - [`CheckTrigger::Startup`] — the boot auto-check. A failure (e.g. no v2
///   release exists yet, or offline) is **logged and ignored**: it must never
///   surface as a scary "Update failed" banner, so we do NOT emit an error
///   `update://status` and leave the cached status untouched (idle/unknown). It
///   also fires the once-per-version update-available notice on success.
/// - [`CheckTrigger::UserRequested`] — the user pressed "Check for updates". A
///   failure IS surfaced, but as a clean human-readable STRING in
///   [`UpdateStatus::error`] (never a serialized [`AppError`] object → the FE can
///   no longer render `[object Object]`), emitted via `update://status`. It does
///   NOT fire a notification (the startup path owns that).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckTrigger {
    Startup,
    UserRequested,
}

/// Query the updater endpoint and build an [`UpdateStatus`], caching the pending
/// [`Update`] for a subsequent [`run_install`]. Emits `update://status` on
/// success (and, for [`CheckTrigger::UserRequested`], on failure — as a clean
/// error string).
///
/// A check failure is handled per [`CheckTrigger`] (see [`handle_check_error`]),
/// so this always returns `Ok`: a user check surfaces its error inside the
/// status (never as an IPC rejection object), and a startup check swallows it.
pub async fn run_check(
    app: &AppHandle,
    app_state: &AppState,
    updater_state: &UpdaterState,
    trigger: CheckTrigger,
) -> Result<UpdateStatus, AppError> {
    // Only the startup/auto path fires the once-per-version available notice.
    let notify_on_available = trigger == CheckTrigger::Startup;

    // Run the check, collapsing any failure into a clean human-readable STRING
    // (never a serialized error object — that is what produced `[object Object]`).
    let check = async {
        let updater = app
            .updater()
            .map_err(|e| format!("updater unavailable: {e}"))?;
        updater
            .check()
            .await
            .map_err(|e| format!("update check failed: {e}"))
    }
    .await;

    let maybe_update = match check {
        Ok(u) => u,
        Err(msg) => return Ok(handle_check_error(app, updater_state, trigger, msg)),
    };

    let last_skipped = app_state.settings_snapshot().last_skipped_version;

    let status = match maybe_update {
        Some(update) => {
            let version = update.version.clone();
            let notes = update.body.clone();
            let skipped = last_skipped.as_deref() == Some(version.as_str());

            // Cache the pending update so install can act without re-checking.
            *updater_state.pending.lock().await = Some(update);

            if notify_on_available && !skipped {
                let mut last_notified = updater_state.lock_notified();
                if last_notified.as_deref() != Some(version.as_str()) {
                    *last_notified = Some(version.clone());
                    // Drop the guard before the (best-effort) notification call.
                    drop(last_notified);
                    crate::platform::notify::notify_update_available(app_state, &version);
                }
            }

            UpdateStatus {
                available: true,
                version: Some(version),
                notes,
                skipped,
                error: None,
            }
        }
        None => {
            // Up to date — clear any stale pending update.
            *updater_state.pending.lock().await = None;
            UpdateStatus::default()
        }
    };

    updater_state.set_latest_status(status.clone());
    emit_status(app, &status);
    Ok(status)
}

/// Surface (or swallow) a check FAILURE per the trigger (BUG 3). Returns the
/// [`UpdateStatus`] the caller should return.
fn handle_check_error(
    app: &AppHandle,
    updater_state: &UpdaterState,
    trigger: CheckTrigger,
    msg: String,
) -> UpdateStatus {
    match trigger {
        CheckTrigger::Startup => {
            // A failed boot check must NEVER surface as a banner: log and ignore,
            // do not emit, and leave the cached status as-is (idle/unknown, or a
            // previously-known-available state — never clobbered by a transient
            // failure).
            tracing::warn!(error = %msg, "startup update-check failed (ignored)");
            updater_state.latest_status()
        }
        CheckTrigger::UserRequested => {
            // Surface a CLEAN STRING (never a serialized object) so the FE never
            // renders `[object Object]`, and emit it so the banner updates.
            tracing::warn!(error = %msg, "user update-check failed");
            let status = UpdateStatus {
                error: Some(msg),
                ..UpdateStatus::default()
            };
            updater_state.set_latest_status(status.clone());
            emit_status(app, &status);
            status
        }
    }
}

/// Download → verify (minisign) → install the pending update, emitting
/// `update://progress`, then relaunch into the new version.
///
/// Requires a prior successful [`run_check`] that found an update (its [`Update`]
/// is cached in [`UpdaterState`]). The minisign signature is verified inside
/// `download_and_install` against the embedded pubkey; a tampered/unsigned
/// bundle returns `Err`, surfaced as [`AppError::Updater`].
pub async fn run_install(app: &AppHandle, updater_state: &UpdaterState) -> Result<(), AppError> {
    let update =
        updater_state.pending.lock().await.take().ok_or_else(|| {
            AppError::Updater("no pending update — run check_update first".into())
        })?;

    let progress_app = app.clone();
    let mut downloaded: u64 = 0;

    let result = update
        .download_and_install(
            move |chunk_len, content_len| {
                downloaded = downloaded.saturating_add(chunk_len as u64);
                let _ = progress_app.emit(
                    events::UPDATE_PROGRESS,
                    UpdateProgress {
                        downloaded,
                        total: content_len,
                    },
                );
            },
            || {
                tracing::info!("update download finished; installing");
            },
        )
        .await;

    match result {
        Ok(()) => {
            tracing::info!("update installed; relaunching");
            app.restart();
        }
        Err(e) => Err(AppError::Updater(format!("update install failed: {e}"))),
    }
}

/// One-shot boot update-check (spec 03 §16). No-op when `autoCheckUpdates` is
/// off. Errors are logged and swallowed — a failed check must never disrupt
/// startup. Notifies once per version on the way through.
pub async fn auto_check_on_startup(
    app: AppHandle,
    app_state: Arc<AppState>,
    updater_state: Arc<UpdaterState>,
) {
    if !app_state.settings_snapshot().auto_check_updates {
        tracing::debug!("auto update-check disabled (autoCheckUpdates=false)");
        return;
    }
    // Startup trigger: a failure is logged-and-ignored inside `run_check` and
    // never emits an error `update://status` (BUG 3) — so a missing v2 release
    // can't surface as a "Update failed" banner.
    match run_check(&app, &app_state, &updater_state, CheckTrigger::Startup).await {
        Ok(status) if status.available => {
            tracing::info!(version = ?status.version, skipped = status.skipped, "update available");
        }
        Ok(_) => tracing::debug!("no update available (or check ignored)"),
        Err(e) => tracing::warn!(error = %e, "startup update-check failed (ignored)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tray::menu::update_notice_from_status;

    fn available(version: &str) -> UpdateStatus {
        UpdateStatus {
            available: true,
            version: Some(version.to_string()),
            notes: None,
            skipped: false,
            error: None,
        }
    }

    /// F51: after skipping the cached version, the cached status flips to
    /// `skipped=true` so the tray notice (`available && !skipped`) hides that
    /// version immediately — no wait for the next `check_update`/restart.
    #[test]
    fn skip_flips_cached_status_and_hides_tray_notice() {
        let state = UpdaterState::new();
        state.set_latest_status(available("2.1.0"));

        // Before skip: the tray offers the notice.
        assert!(update_notice_from_status(&state.latest_status()).is_some());

        let refreshed = state.mark_skipped("2.1.0");

        // The returned + cached status both mark it skipped...
        assert!(refreshed.skipped);
        assert!(state.latest_status().skipped);
        // ...and the tray notice is now hidden for that version.
        assert!(update_notice_from_status(&state.latest_status()).is_none());
    }

    /// A skip for a DIFFERENT version than the one cached must NOT flip the flag
    /// on the current offer (stale-skip guard).
    #[test]
    fn skip_of_other_version_leaves_current_offer_visible() {
        let state = UpdaterState::new();
        state.set_latest_status(available("2.1.0"));

        let refreshed = state.mark_skipped("2.0.0");

        assert!(!refreshed.skipped);
        assert!(!state.latest_status().skipped);
        assert!(update_notice_from_status(&state.latest_status()).is_some());
    }
}
