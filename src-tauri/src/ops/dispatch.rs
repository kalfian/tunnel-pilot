//! Command dispatcher for the web transport (spec 02 §10).
//!
//! Maps a §6 command name plus the exact JSON args object Tauri's `invoke` would
//! receive onto the shared `ops::*` function, and serializes the return value.
//! The desktop app does not use this (Tauri's `generate_handler!` is its
//! dispatcher); keeping both bindings on the same `ops` functions is what keeps
//! the two products feature-identical.
//!
//! Desktop-only commands (window, tray, updater, PATH shim, path-based backup)
//! are deliberately absent and answer `notFound`, exactly like an unknown name.
//!
//! Security: argument structs that carry a secret (`set_forward_password`,
//! `save_variable`'s `VariableInput`) do not derive `Debug`, and nothing here logs args or results (AGENTS §8).

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;
use crate::ops::{backup, forwards, groups, logs, settings, variables};
use crate::ssh::engine;
use crate::state::models::{
    AppSettings, ForwardConfig, ForwardInput, GroupInput, UpdateStatus, VariableEndpointInput,
    VariableInput,
};
use crate::state::AppState;
use crate::storage::backup::ImportMode;

/// Every command the web transport serves (02 §10). Kept in one list so the
/// docs, the tests and the dispatcher cannot drift apart.
pub const WEB_COMMANDS: &[&str] = &[
    // Forwards (§6.1)
    "list_forwards",
    "create_forward",
    "update_forward",
    "delete_forward",
    "duplicate_forward",
    "reorder_forwards",
    "connect_forward",
    "disconnect_forward",
    "retry_forward",
    "start_all",
    "stop_all",
    "get_forward_runtime",
    "copy_ssh_command",
    "set_forward_password",
    "clear_forward_password",
    "trust_host_key",
    "forget_host_key",
    // Groups & tags (§6.2)
    "list_groups",
    "create_group",
    "update_group",
    "delete_group",
    "assign_forward_group",
    "start_group",
    "stop_group",
    "list_tags",
    // Variables (§6.9)
    "list_variables",
    "save_variable",
    "delete_variable",
    "allow_variable_endpoint",
    "revoke_variable_endpoint",
    // Settings (§6.3)
    "get_settings",
    "update_settings",
    // Logs (§6.4)
    "get_logs",
    "clear_logs",
    "get_logs_text",
    // Backup (§6.5, text variants)
    "export_backup_text",
    "import_backup_text",
    // App (§6.7)
    "app_hydrate",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdArgs {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InputArgs<T> {
    input: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IdInputArgs<T> {
    id: String,
    input: T,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReorderArgs {
    ordered_ids: Vec<String>,
}

/// Carries a secret: no `Debug`, never logged.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PasswordArgs {
    id: String,
    password: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NameArgs {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RevokeEndpointArgs {
    name: String,
    endpoint: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AssignArgs {
    forward_id: String,
    #[serde(default)]
    group_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupIdArgs {
    group_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportTextArgs {
    content: String,
    mode: ImportMode,
}

/// Parse the args object for a command. `null`/absent args parse as `{}`.
fn args<T: DeserializeOwned>(command: &str, value: Value) -> Result<T, AppError> {
    let value = if value.is_null() {
        Value::Object(Default::default())
    } else {
        value
    };
    serde_json::from_value(value)
        .map_err(|e| AppError::InvalidInput(format!("bad arguments for {command}: {e}")))
}

fn json<T: Serialize>(value: T) -> Result<Value, AppError> {
    serde_json::to_value(value).map_err(|e| AppError::Serialization(e.to_string()))
}

/// Update status reported to the web UI: the server edition has no self-updater
/// (03 §22), so it is always "nothing available".
fn idle_update_status() -> UpdateStatus {
    UpdateStatus {
        available: false,
        version: None,
        notes: None,
        skipped: false,
        error: None,
    }
}

// --- Secret variables over the web (spec 03 §24) ---
//
// A web session may not know a secret variable's value, so every web action
// that could send one somewhere new — a forward created with it, a forward
// switched to it or moved to another server, and trusting or forgetting the
// host key of a forward that uses it (a replaced key is a new server) —
// consumes a one-time grant from `allow_variable_endpoint` (proof of value).
// The desktop app and the CLI never come through here.

fn trimmed(name: Option<&str>) -> Option<&str> {
    name.map(str::trim).filter(|n| !n.is_empty())
}

fn take_grant(
    state: &Arc<AppState>,
    scope: &str,
    name: &str,
    host: &str,
    port: u16,
) -> Result<(), AppError> {
    let endpoint = variables::endpoint_key(host, port);
    if state.web_guard.take_grant(scope, name, &endpoint) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "confirm {name} for {endpoint} by entering its value"
        )))
    }
}

fn web_binding_for_create(
    state: &Arc<AppState>,
    scope: &str,
    input: &ForwardInput,
) -> Result<variables::Binding, AppError> {
    match trimmed(input.password_variable.as_deref()) {
        Some(name) => {
            take_grant(state, scope, name, &input.ssh_host, input.ssh_port)?;
            Ok(variables::Binding::BindAfterSave)
        }
        None => Ok(variables::Binding::Required),
    }
}

/// No proof only when the edit keeps the same variable on the very same
/// server — compared RAW, the way `update` decides whether to keep the pin —
/// that is still bound and already pinned. Anything else could send the
/// secret somewhere new (a host spelled differently drops the pin).
fn web_binding_for_update(
    state: &Arc<AppState>,
    scope: &str,
    id: &str,
    input: &ForwardInput,
) -> Result<variables::Binding, AppError> {
    let existing = state
        .get_config(id)
        .ok_or_else(|| AppError::NotFound(format!("forward {id}")))?;
    let Some(name) = trimmed(input.password_variable.as_deref()) else {
        return Ok(variables::Binding::Required);
    };
    let endpoint = variables::endpoint_key(&input.ssh_host, input.ssh_port);
    let unchanged = existing.password_variable.as_deref() == Some(name)
        && existing.ssh_host == input.ssh_host
        && existing.ssh_port == input.ssh_port
        && existing.host_key.is_some()
        && state
            .get_variable(name)
            .is_some_and(|v| v.allowed_endpoints.contains(&endpoint));
    if unchanged {
        return Ok(variables::Binding::Required);
    }
    take_grant(state, scope, name, &input.ssh_host, input.ssh_port)?;
    Ok(variables::Binding::BindAfterSave)
}

fn require_grant_for_forward(state: &Arc<AppState>, scope: &str, id: &str) -> Result<(), AppError> {
    let Some(cfg) = state.get_config(id) else {
        return Ok(()); // the command itself reports `notFound`
    };
    match trimmed(cfg.password_variable.as_deref()) {
        Some(name) => take_grant(state, scope, name, &cfg.ssh_host, cfg.ssh_port),
        None => Ok(()),
    }
}

/// A forward that would send a secret variable to a server whose key is not
/// pinned yet: its first dial trusts whatever key answers, so from the web it
/// needs a permit (given by the proof-carrying save of that forward).
fn first_dial_blocked(state: &Arc<AppState>, scope: &str, cfg: &ForwardConfig) -> bool {
    let sends_secret = trimmed(cfg.password_variable.as_deref())
        .and_then(|n| state.get_variable(n))
        .is_some_and(|v| v.secret);
    sends_secret && cfg.host_key.is_none() && !state.web_guard.may_first_dial(scope, &cfg.id)
}

fn require_dial_permission(state: &Arc<AppState>, scope: &str, id: &str) -> Result<(), AppError> {
    let Some(cfg) = state.get_config(id) else {
        return Ok(());
    };
    if first_dial_blocked(state, scope, &cfg) {
        return Err(AppError::InvalidInput(format!(
            "{} has not connected before, so its server's key is not pinned yet and it would \
             send {} to whatever answers: connect it once from the desktop app or the CLI, or \
             edit it here and confirm the variable's value",
            cfg.name,
            cfg.password_variable.as_deref().unwrap_or_default()
        )));
    }
    Ok(())
}

/// Bulk starts from the web leave out forwards they may not dial yet (logged,
/// so the Activity view says why).
fn skip_first_dial(state: &Arc<AppState>, scope: &str, cfg: &ForwardConfig) -> bool {
    let skip = first_dial_blocked(state, scope, cfg);
    if skip {
        tracing::warn!(
            tunnel = %cfg.name,
            "not started from the web: it would send a secret variable to a server whose key \
             is not pinned yet (connect it once from the desktop app or the CLI)"
        );
    }
    skip
}

/// Run one command on behalf of the web session `scope` (an opaque session
/// key: grants and permits are only usable by the session that earned them).
/// Unknown and desktop-only names are `notFound`.
pub async fn invoke_scoped(
    state: &Arc<AppState>,
    scope: &str,
    command: &str,
    raw: Value,
) -> Result<Value, AppError> {
    match command {
        // --- Forwards ---
        "list_forwards" => json(state.configs_snapshot()),
        "create_forward" => {
            let a: InputArgs<ForwardInput> = args(command, raw)?;
            let binding = web_binding_for_create(state, scope, &a.input)?;
            let cfg = forwards::create_as(state, a.input, binding).await?;
            if binding == variables::Binding::BindAfterSave {
                state.web_guard.permit_first_dial(scope, &cfg.id);
            }
            json(cfg)
        }
        "update_forward" => {
            let a: IdInputArgs<ForwardInput> = args(command, raw)?;
            let binding = web_binding_for_update(state, scope, &a.id, &a.input)?;
            let cfg = forwards::update_as(state, a.id, a.input, binding).await?;
            if binding == variables::Binding::BindAfterSave {
                state.web_guard.permit_first_dial(scope, &cfg.id);
            }
            json(cfg)
        }
        "delete_forward" => {
            let a: IdArgs = args(command, raw)?;
            json(forwards::delete(state, &a.id).await?)
        }
        "duplicate_forward" => {
            let a: IdArgs = args(command, raw)?;
            json(forwards::duplicate(state, &a.id).await?)
        }
        "reorder_forwards" => {
            let a: ReorderArgs = args(command, raw)?;
            json(forwards::reorder(state, &a.ordered_ids).await?)
        }
        "connect_forward" => {
            let a: IdArgs = args(command, raw)?;
            require_dial_permission(state, scope, &a.id)?;
            json(engine::connect_forward(state, &a.id).await?)
        }
        "disconnect_forward" => {
            let a: IdArgs = args(command, raw)?;
            json(engine::disconnect_forward(state, &a.id, true).await?)
        }
        "retry_forward" => {
            let a: IdArgs = args(command, raw)?;
            require_dial_permission(state, scope, &a.id)?;
            json(engine::retry_forward(state, &a.id).await?)
        }
        "start_all" => {
            json(forwards::run_start_all_where(state, |c| !skip_first_dial(state, scope, c)).await?)
        }
        "stop_all" => json(forwards::run_stop_all(state).await?),
        "get_forward_runtime" => {
            let a: IdArgs = args(command, raw)?;
            json(forwards::runtime_or_default(state, &a.id))
        }
        "copy_ssh_command" => {
            let a: IdArgs = args(command, raw)?;
            let cfg = state
                .get_config(&a.id)
                .ok_or_else(|| AppError::NotFound(format!("forward {}", a.id)))?;
            json(forwards::ssh_command(state, &cfg))
        }
        "set_forward_password" => {
            let a: PasswordArgs = args(command, raw)?;
            json(forwards::set_password(state, &a.id, a.password).await?)
        }
        "clear_forward_password" => {
            let a: IdArgs = args(command, raw)?;
            json(forwards::clear_password(state, &a.id).await?)
        }
        "trust_host_key" => {
            let a: IdArgs = args(command, raw)?;
            require_grant_for_forward(state, scope, &a.id)?;
            json(forwards::trust_host_key(state, &a.id).await?)
        }
        "forget_host_key" => {
            let a: IdArgs = args(command, raw)?;
            require_grant_for_forward(state, scope, &a.id)?;
            json(forwards::forget_host_key(state, &a.id).await?)
        }
        // --- Groups & tags ---
        "list_groups" => json(groups::list_groups(state)),
        "create_group" => {
            let a: InputArgs<GroupInput> = args(command, raw)?;
            json(groups::create_group(state, a.input).await?)
        }
        "update_group" => {
            let a: IdInputArgs<GroupInput> = args(command, raw)?;
            json(groups::update_group(state, a.id, a.input).await?)
        }
        "delete_group" => {
            let a: IdArgs = args(command, raw)?;
            json(groups::delete_group(state, a.id).await?)
        }
        "assign_forward_group" => {
            let a: AssignArgs = args(command, raw)?;
            json(groups::assign_forward_group(state, a.forward_id, a.group_id).await?)
        }
        "start_group" => {
            let a: GroupIdArgs = args(command, raw)?;
            json(
                groups::start_group_where(state, a.group_id, |c| !skip_first_dial(state, scope, c))
                    .await?,
            )
        }
        "stop_group" => {
            let a: GroupIdArgs = args(command, raw)?;
            json(groups::stop_group(state, a.group_id).await?)
        }
        "list_tags" => json(groups::list_tags(state)),
        // --- Variables (a secret value only travels inbound) ---
        "list_variables" => json(variables::list(state)),
        "save_variable" => {
            let a: InputArgs<VariableInput> = args(command, raw)?;
            json(variables::save(state, a.input).await?)
        }
        "delete_variable" => {
            let a: NameArgs = args(command, raw)?;
            json(variables::delete(state, &a.name).await?)
        }
        // Over the web a secret variable is bound to a new server only with
        // proof of its value; create/update never bind (spec 03 §24).
        "allow_variable_endpoint" => {
            let a: InputArgs<VariableEndpointInput> = args(command, raw)?;
            json(variables::allow_endpoint_with_proof(state, scope, a.input).await?)
        }
        "revoke_variable_endpoint" => {
            let a: RevokeEndpointArgs = args(command, raw)?;
            json(variables::revoke_endpoint(state, &a.name, &a.endpoint).await?)
        }
        // --- Settings ---
        "get_settings" => json(settings::get_settings(state)),
        "update_settings" => {
            let a: InputArgs<AppSettings> = args(command, raw)?;
            json(settings::update_settings(state, a.input).await?)
        }
        // --- Logs ---
        "get_logs" => json(logs::get_logs(state)),
        "clear_logs" => {
            logs::clear_logs(state);
            Ok(Value::Null)
        }
        "get_logs_text" => json(logs::get_logs_text(state)),
        // --- Backup (text variants only: a browser cannot name a server path) ---
        "export_backup_text" => json(backup::export_backup_text(state)?),
        "import_backup_text" => {
            let a: ImportTextArgs = args(command, raw)?;
            json(backup::import_backup_text(state, a.content, a.mode).await?)
        }
        // --- App ---
        "app_hydrate" => json(state.app_snapshot(idle_update_status())),
        _ => Err(AppError::NotFound(format!(
            "unknown command '{command}' (desktop-only commands are not served over the web)"
        ))),
    }
}

/// Test shorthand: one fixed web session.
#[cfg(test)]
pub async fn invoke(state: &Arc<AppState>, command: &str, raw: Value) -> Result<Value, AppError> {
    invoke_scoped(state, "test-session", command, raw).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json as j;

    fn state() -> Arc<AppState> {
        Arc::new(AppState::new_headless())
    }

    fn input(name: &str) -> Value {
        j!({
            "name": name,
            "sshHost": "bastion.example.com",
            "sshPort": 22,
            "sshUsername": "deploy",
            "identityFilePath": "/keys/id",
            "localBindAddress": "127.0.0.1",
            "localPort": 15432,
            "remoteHost": "db.internal",
            "remotePort": 5432,
            "keepAliveIntervalSec": 30,
            "keepAliveMaxCount": 5,
            "groupId": null,
            "tags": []
        })
    }

    #[tokio::test]
    async fn every_listed_command_is_dispatched() {
        // A command in WEB_COMMANDS must never fall through to the unknown arm:
        // with empty args it may fail argument parsing, but never with the
        // "unknown command" notFound.
        let state = state();
        for cmd in WEB_COMMANDS {
            if let Err(AppError::NotFound(msg)) = invoke(&state, cmd, Value::Null).await {
                assert!(
                    !msg.contains("unknown command"),
                    "{cmd} is listed but not dispatched"
                );
            }
        }
    }

    #[tokio::test]
    async fn desktop_only_and_unknown_commands_are_not_found() {
        let state = state();
        for cmd in [
            "show_window",
            "quit_app",
            "install_update",
            "install_cli_shim",
            "export_backup",
            "import_backup",
            "nope",
        ] {
            assert!(
                matches!(
                    invoke(&state, cmd, Value::Null).await,
                    Err(AppError::NotFound(_))
                ),
                "{cmd} must not be served"
            );
        }
    }

    #[tokio::test]
    async fn crud_round_trip_with_tauri_shaped_args() {
        let state = state();
        let created = invoke(&state, "create_forward", j!({ "input": input("db") }))
            .await
            .expect("create");
        let id = created["id"].as_str().expect("id").to_string();
        assert_eq!(created["hasStoredPassword"], j!(false));

        let listed = invoke(&state, "list_forwards", j!({})).await.expect("list");
        assert_eq!(listed.as_array().map(Vec::len), Some(1));

        let ssh = invoke(&state, "copy_ssh_command", j!({ "id": id }))
            .await
            .expect("ssh");
        assert!(ssh.as_str().is_some_and(|s| s.starts_with("ssh -N -L")));

        invoke(
            &state,
            "set_forward_password",
            j!({ "id": id, "password": "s3cret" }),
        )
        .await
        .expect("set password");
        let hydrated = invoke(&state, "app_hydrate", Value::Null)
            .await
            .expect("hydrate");
        let text = hydrated.to_string();
        assert!(!text.contains("s3cret"), "no command may return a secret");
        assert_eq!(hydrated["forwards"][0]["hasStoredPassword"], j!(true));

        let backup = invoke(&state, "export_backup_text", Value::Null)
            .await
            .expect("export");
        assert!(
            !backup.to_string().contains("s3cret"),
            "backups carry no secret"
        );

        invoke(&state, "delete_forward", j!({ "id": id }))
            .await
            .expect("delete");
        let listed = invoke(&state, "list_forwards", Value::Null)
            .await
            .expect("list");
        assert_eq!(listed.as_array().map(Vec::len), Some(0));
    }

    #[tokio::test]
    async fn variables_over_the_web_never_return_a_secret() {
        let state = state();
        invoke(
            &state,
            "save_variable",
            j!({ "input": { "name": "DB_USER", "secret": false, "value": "deploy" } }),
        )
        .await
        .expect("plain");
        let saved = invoke(
            &state,
            "save_variable",
            j!({ "input": { "name": "DB_PASS", "secret": true, "value": "w3b-s3cret" } }),
        )
        .await
        .expect("secret");
        assert_eq!(saved["hasStoredValue"], j!(true));
        assert!(!saved.to_string().contains("w3b-s3cret"));

        let mut fwd = input("db");
        fwd["sshUsername"] = j!("{{DB_USER}}");
        fwd["identityFilePath"] = j!(null);
        fwd["passwordVariable"] = j!("DB_PASS");
        // Over the web, a secret variable needs proof of its value before it
        // may be sent to a new server.
        assert!(matches!(
            invoke(&state, "create_forward", j!({ "input": fwd.clone() })).await,
            Err(AppError::InvalidInput(_))
        ));
        let proof = |value: &str| {
            j!({ "input": { "name": "DB_PASS", "sshHost": "bastion.example.com",
                            "sshPort": 22, "value": value } })
        };
        assert!(invoke(&state, "allow_variable_endpoint", proof("wrong"))
            .await
            .is_err());
        // Locked out after a wrong value, even for the right one.
        assert!(
            invoke(&state, "allow_variable_endpoint", proof("w3b-s3cret"))
                .await
                .is_err()
        );
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        let bound = invoke(&state, "allow_variable_endpoint", proof("w3b-s3cret"))
            .await
            .expect("proof");
        assert!(!bound.to_string().contains("w3b-s3cret"));
        let created = invoke(&state, "create_forward", j!({ "input": fwd.clone() }))
            .await
            .expect("create with variables");
        let id = created["id"].as_str().expect("id").to_string();

        // The grant was used up: a second forward needs a new proof.
        let mut second = fwd.clone();
        second["name"] = j!("db2");
        second["localPort"] = j!(15433);
        assert!(invoke(&state, "create_forward", j!({ "input": second }))
            .await
            .is_err());
        // The proof-carrying save let THIS session make the first dial of the
        // unpinned forward; another session may not.
        assert!(require_dial_permission(&state, "test-session", &id).is_ok());
        assert!(require_dial_permission(&state, "other-session", &id).is_err());
        let cfg = state.get_config(&id).expect("cfg");
        assert!(first_dial_blocked(&state, "other-session", &cfg));

        // Once pinned, an edit that keeps the variable and the server needs no
        // proof...
        let mut pinned = cfg;
        pinned.host_key = Some(crate::state::models::HostKeyPin {
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:x".into(),
            pinned_at: None,
        });
        state.upsert_config(pinned);
        assert!(require_dial_permission(&state, "other-session", &id).is_ok());
        // ...but the same host spelled differently drops the pin, so it does.
        let mut recased = fwd.clone();
        recased["sshHost"] = j!("BASTION.example.com");
        assert!(
            invoke(&state, "update_forward", j!({ "id": id, "input": recased }))
                .await
                .is_err(),
            "a case-only host change must need a proof"
        );
        let mut renamed = fwd.clone();
        renamed["name"] = j!("db-renamed");
        invoke(&state, "update_forward", j!({ "id": id, "input": renamed }))
            .await
            .expect("rename");
        // ...moving it to another server, or trusting / forgetting its host
        // key (a new server, in effect), does.
        let mut moved = fwd.clone();
        moved["sshHost"] = j!("evil.example");
        assert!(
            invoke(&state, "update_forward", j!({ "id": id, "input": moved }))
                .await
                .is_err()
        );
        for cmd in ["trust_host_key", "forget_host_key"] {
            assert!(
                matches!(
                    invoke(&state, cmd, j!({ "id": id })).await,
                    Err(AppError::InvalidInput(ref m)) if m.contains("confirm DB_PASS")
                ),
                "{cmd} must need a proof over the web"
            );
        }
        let ssh = invoke(&state, "copy_ssh_command", j!({ "id": id }))
            .await
            .expect("ssh");
        assert!(ssh
            .as_str()
            .is_some_and(|s| s.ends_with("deploy@bastion.example.com")));

        for cmd in [
            "list_variables",
            "app_hydrate",
            "export_backup_text",
            "list_forwards",
        ] {
            let out = invoke(&state, cmd, Value::Null).await.expect(cmd);
            assert!(
                !out.to_string().contains("w3b-s3cret"),
                "{cmd} leaked a secret"
            );
        }
        assert!(matches!(
            invoke(&state, "delete_variable", j!({ "name": "DB_PASS" })).await,
            Err(AppError::InvalidInput(_))
        ));
        // An older client that does not send `passwordVariable` still works.
        invoke(&state, "create_forward", j!({ "input": input("legacy") }))
            .await
            .expect("legacy payload");
    }

    #[tokio::test]
    async fn bad_args_are_invalid_input() {
        let state = state();
        assert!(matches!(
            invoke(&state, "delete_forward", j!({ "nope": 1 })).await,
            Err(AppError::InvalidInput(_))
        ));
    }
}
