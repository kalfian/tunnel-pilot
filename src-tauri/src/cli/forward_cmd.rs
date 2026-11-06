//! Client side of the forward CRUD commands: `add`, `edit`, `remove`,
//! `set-password`, `clear-password` (spec 03 §20).
//!
//! The owner (desktop app or headless daemon) does every config mutation
//! through the control socket. The one thing it never receives is a password:
//! THIS process reads it (no-echo prompt or stdin), writes it straight into the
//! credential store — the OS keychain, or the 0600 fallback file the owner
//! reads too — and then sends `password-set`, which only flips the
//! `hasStoredPassword` flag. A secret therefore never crosses the socket, never
//! appears in argv, and is never printed (AGENTS §8).

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::args::{Command, Invocation, PasswordSource};
use crate::cli::client::{report, report_client_error, send, EXIT_INTERNAL, EXIT_USAGE};
use crate::cli::protocol::{
    CliCommand, CliData, CliRequest, CliResponse, ForwardFields, SaveChange,
};
use crate::credentials::CredentialStore;
use crate::error::AppError;

/// Every CRUD round-trip is quick (an `edit` of a live forward waits for its
/// teardown, which the engine bounds well below this).
const CRUD_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Run one CRUD command against the socket at `path`.
pub fn run(invocation: &Invocation, path: &Path) -> i32 {
    let outcome = match &invocation.command {
        Command::Add { fields, password } => add(path, fields, *password),
        Command::Edit {
            target,
            fields,
            password,
        } => edit(path, target, fields, *password),
        Command::Remove { target } => simple(path, CliCommand::Remove, target),
        Command::SetPassword { target, source } => set_password(path, target, *source),
        Command::ClearPassword { target } => simple(path, CliCommand::PasswordClear, target),
        Command::TrustHostKey { target } => simple(path, CliCommand::HostKeyTrust, target),
        Command::ForgetHostKey { target } => simple(path, CliCommand::HostKeyForget, target),
        _ => Err(EXIT_INTERNAL),
    };
    match outcome {
        Ok(response) => report_crud(invocation, response),
        Err(code) => code,
    }
}

/// `report` with one CRUD-specific twist: a validation failure of `add`/`edit`
/// is a usage error (2), not "target not found" (4).
fn report_crud(invocation: &Invocation, response: CliResponse) -> i32 {
    let is_field_command = matches!(
        invocation.command,
        Command::Add { .. } | Command::Edit { .. }
    );
    let validation_failure = is_field_command
        && matches!(
            response.error,
            Some(AppError::InvalidInput(ref m)) if !m.starts_with("ambiguous target")
        );
    let code = report(invocation, response);
    if validation_failure {
        EXIT_USAGE
    } else {
        code
    }
}

fn round_trip(path: &Path, request: &CliRequest) -> Result<CliResponse, i32> {
    send(path, request, CRUD_READ_TIMEOUT)
        .map_err(|e| report_client_error(path, CRUD_READ_TIMEOUT, e))
}

fn targeted(cmd: CliCommand, target: &str) -> CliRequest {
    CliRequest {
        target: Some(target.to_string()),
        ..CliRequest::new(cmd)
    }
}

fn simple(path: &Path, cmd: CliCommand, target: &str) -> Result<CliResponse, i32> {
    round_trip(path, &targeted(cmd, target))
}

fn add(path: &Path, fields: &ForwardFields, password: PasswordSource) -> Result<CliResponse, i32> {
    let name = fields.name.clone().unwrap_or_default();
    // Read the secret BEFORE creating anything, so a cancelled prompt never
    // leaves a forward behind that cannot authenticate.
    let secret = read_password(password, &name)?;
    let fields = with_absolute_identity(fields.clone())?;
    let request = CliRequest {
        forward: Some(fields),
        ..CliRequest::new(CliCommand::Add)
    };
    let created = round_trip(path, &request)?;
    finish_with_password(path, created, secret)
}

fn edit(
    path: &Path,
    target: &str,
    fields: &ForwardFields,
    password: PasswordSource,
) -> Result<CliResponse, i32> {
    let secret = read_password(password, target)?;
    let mut fields = with_absolute_identity(fields.clone())?;
    if secret.is_some() {
        // Switching to password auth: the engine prefers an identity file when
        // both exist (ssh/client.rs), so the key must go.
        fields.identity_file_path = Some(String::new());
    }
    let switching_to_key = fields
        .identity_file_path
        .as_deref()
        .is_some_and(|p| !p.is_empty());

    let request = CliRequest {
        target: Some(target.to_string()),
        forward: (!fields.is_empty()).then_some(fields),
        ..CliRequest::new(CliCommand::Edit)
    };
    // A password-only edit has no fields to send: resolve via `status` instead.
    let response = if request.forward.is_some() {
        round_trip(path, &request)?
    } else {
        round_trip(path, &targeted(CliCommand::Status, target))?
    };
    let Some(view) = saved_or_status_view(&response) else {
        return Ok(response);
    };
    let (id, had_password) = (view.0, view.1);

    if switching_to_key && had_password {
        // Key auth replaces the stored password (the desktop form's rule).
        let cleared = round_trip(path, &targeted(CliCommand::PasswordClear, &id))?;
        if !cleared.ok {
            return Ok(cleared);
        }
        return Ok(relabel(cleared, SaveChange::Updated));
    }
    let response = relabel(response, SaveChange::Updated);
    finish_with_password(path, response, secret)
}

fn set_password(path: &Path, target: &str, source: PasswordSource) -> Result<CliResponse, i32> {
    // Resolve first: a typo must fail before the user types a secret.
    let status = round_trip(path, &targeted(CliCommand::Status, target))?;
    let Some(CliData::Status { forward, .. }) = &status.data else {
        return Ok(status);
    };
    let (id, name) = (forward.id.clone(), forward.name.clone());
    let uses_key = forward
        .identity_file_path
        .as_deref()
        .is_some_and(|p| !p.is_empty());
    let secret = read_password(source, &name)?;
    if uses_key {
        // The engine would keep using the key; switch the forward to password
        // auth so the new secret is actually used.
        let request = CliRequest {
            target: Some(id.clone()),
            forward: Some(ForwardFields {
                identity_file_path: Some(String::new()),
                ..ForwardFields::default()
            }),
            ..CliRequest::new(CliCommand::Edit)
        };
        let edited = round_trip(path, &request)?;
        if !edited.ok {
            return Ok(edited);
        }
    }
    let Some(secret) = secret else {
        return Err(EXIT_USAGE);
    };
    store_password(path, &id, secret)
}

/// After a successful `add`/`edit`, store the secret (if any) and report the
/// final view under the original change label.
fn finish_with_password(
    path: &Path,
    response: CliResponse,
    secret: Option<String>,
) -> Result<CliResponse, i32> {
    let Some(secret) = secret else {
        return Ok(response);
    };
    let Some(CliData::Saved { forward, change }) = &response.data else {
        return Ok(response);
    };
    let (id, name, change) = (forward.id.clone(), forward.name.clone(), *change);
    match store_password(path, &id, secret) {
        Ok(stored) if stored.ok => Ok(relabel(stored, change)),
        Ok(failed) => Ok(failed),
        Err(code) => {
            eprintln!(
                "note: '{name}' was saved WITHOUT a password; retry with: \
                 {} set-password {id}",
                crate::cli::PROGRAM
            );
            Err(code)
        }
    }
}

/// Write the secret into the credential store from this process, then flip the
/// owner's `hasStoredPassword` flag. Returns the `password-set` response.
fn store_password(path: &Path, id: &str, secret: String) -> Result<CliResponse, i32> {
    let Some(config_dir) = crate::cli::default_config_dir() else {
        eprintln!(
            "error: could not resolve the config dir; set {}",
            crate::cli::CONFIG_DIR_ENV
        );
        return Err(EXIT_INTERNAL);
    };
    let store = CredentialStore::from_app_dir(&config_dir);
    if let Err(e) = store.set_password(id, &secret) {
        eprintln!("error: could not store the password: {e}");
        return Err(EXIT_INTERNAL);
    }
    if !store.keychain_available() {
        eprintln!(
            "note: no OS keychain here; the password is in {} (owner-only, 0600)",
            store.fallback_path().display()
        );
    }
    round_trip(path, &targeted(CliCommand::PasswordSet, id))
}

/// `(id, hasStoredPassword)` from a `Saved` or `Status` payload.
fn saved_or_status_view(response: &CliResponse) -> Option<(String, bool)> {
    match &response.data {
        Some(CliData::Saved { forward, .. }) | Some(CliData::Status { forward, .. }) => {
            Some((forward.id.clone(), forward.has_stored_password))
        }
        _ => None,
    }
}

/// Re-label a successful `Saved`/`Status` response with the change the user
/// actually asked for (e.g. an `add` whose last round-trip was `password-set`).
fn relabel(response: CliResponse, change: SaveChange) -> CliResponse {
    match response.data {
        Some(CliData::Saved { forward, .. }) | Some(CliData::Status { forward, .. }) => {
            CliResponse::ok(CliData::Saved { forward, change })
        }
        _ => response,
    }
}

/// Read a password per `source`. `Ok(None)` for [`PasswordSource::None`].
fn read_password(source: PasswordSource, label: &str) -> Result<Option<String>, i32> {
    let secret = match source {
        PasswordSource::None => return Ok(None),
        PasswordSource::Prompt => {
            match rpassword::prompt_password(format!("SSH password for '{label}': ")) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!(
                        "error: cannot prompt for a password ({e}); \
                         pipe it in with --password-stdin instead"
                    );
                    return Err(EXIT_USAGE);
                }
            }
        }
        PasswordSource::Stdin => {
            let mut line = String::new();
            if let Err(e) = std::io::stdin().lock().read_line(&mut line) {
                eprintln!("error: could not read the password from stdin: {e}");
                return Err(EXIT_INTERNAL);
            }
            line.trim_end_matches(['\n', '\r']).to_string()
        }
    };
    if secret.is_empty() {
        eprintln!("error: the password is empty");
        return Err(EXIT_USAGE);
    }
    Ok(Some(secret))
}

/// The owner (often a daemon started by systemd/launchd with a different cwd
/// and no shell) loads the key file itself, so hand it an absolute path with
/// `~` already expanded. A missing file is a warning, not an error: the key
/// may live on a volume that is mounted later.
fn with_absolute_identity(mut fields: ForwardFields) -> Result<ForwardFields, i32> {
    let Some(raw) = fields.identity_file_path.clone() else {
        return Ok(fields);
    };
    if raw.is_empty() {
        return Ok(fields);
    }
    let path = absolute_path(&raw);
    if !path.is_file() {
        eprintln!(
            "warning: identity file {} does not exist (yet)",
            path.display()
        );
    }
    fields.identity_file_path = Some(path.to_string_lossy().into_owned());
    Ok(fields)
}

/// Expand a leading `~` / `~/` against `$HOME` and anchor a relative path at
/// the current directory. Pure apart from reading `$HOME` / the cwd.
pub fn absolute_path(raw: &str) -> PathBuf {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty());
    let expanded = match (raw.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            PathBuf::from(home).join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(raw),
    };
    if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(&expanded))
            .unwrap_or(expanded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_path_expands_home_and_anchors_relative_paths() {
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            assert_eq!(
                absolute_path("~/.ssh/id_ed25519"),
                PathBuf::from(&home).join(".ssh/id_ed25519")
            );
            assert_eq!(absolute_path("~"), PathBuf::from(&home));
        }
        assert_eq!(absolute_path("/etc/key"), PathBuf::from("/etc/key"));
        assert!(absolute_path("rel/key").is_absolute());
        // `~user` is not expanded (no passwd lookup); it stays relative → cwd.
        assert!(absolute_path("~other/key").ends_with("~other/key"));
    }

    #[test]
    fn relabel_turns_a_password_set_into_the_requested_change() {
        use crate::cli::protocol::{sample_config, ForwardView};
        use crate::state::models::{ForwardRuntime, ForwardStatus, TunnelStats};
        let view = ForwardView::from_parts(
            &sample_config("id-1", "db"),
            &ForwardRuntime {
                status: ForwardStatus::Disconnected,
                stats: TunnelStats::default(),
                last_error: None,
                host_key_mismatch: None,
            },
        );
        let res = CliResponse::ok(CliData::Saved {
            forward: view,
            change: SaveChange::PasswordSet,
        });
        match relabel(res, SaveChange::Created).data {
            Some(CliData::Saved { change, .. }) => assert_eq!(change, SaveChange::Created),
            other => panic!("unexpected {other:?}"),
        }
    }
}
