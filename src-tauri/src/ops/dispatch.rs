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
//! Security: argument structs that carry a secret (`set_forward_password`) do
//! not derive `Debug`, and nothing here logs args or results (AGENTS §8).

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;
use crate::ops::{backup, forwards, groups, logs, settings};
use crate::ssh::engine;
use crate::state::models::{AppSettings, ForwardInput, GroupInput, UpdateStatus};
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

/// Run one command. Unknown and desktop-only names are `notFound`.
pub async fn invoke(state: &Arc<AppState>, command: &str, raw: Value) -> Result<Value, AppError> {
    match command {
        // --- Forwards ---
        "list_forwards" => json(state.configs_snapshot()),
        "create_forward" => {
            let a: InputArgs<ForwardInput> = args(command, raw)?;
            json(forwards::create(state, a.input).await?)
        }
        "update_forward" => {
            let a: IdInputArgs<ForwardInput> = args(command, raw)?;
            json(forwards::update(state, a.id, a.input).await?)
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
            json(engine::connect_forward(state, &a.id).await?)
        }
        "disconnect_forward" => {
            let a: IdArgs = args(command, raw)?;
            json(engine::disconnect_forward(state, &a.id, true).await?)
        }
        "retry_forward" => {
            let a: IdArgs = args(command, raw)?;
            json(engine::retry_forward(state, &a.id).await?)
        }
        "start_all" => json(forwards::run_start_all(state).await?),
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
            json(forwards::build_ssh_command(&cfg))
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
            json(forwards::trust_host_key(state, &a.id).await?)
        }
        "forget_host_key" => {
            let a: IdArgs = args(command, raw)?;
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
            json(groups::start_group(state, a.group_id).await?)
        }
        "stop_group" => {
            let a: GroupIdArgs = args(command, raw)?;
            json(groups::stop_group(state, a.group_id).await?)
        }
        "list_tags" => json(groups::list_tags(state)),
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
    async fn bad_args_are_invalid_input() {
        let state = state();
        assert!(matches!(
            invoke(&state, "delete_forward", j!({ "nope": 1 })).await,
            Err(AppError::InvalidInput(_))
        ));
    }
}
