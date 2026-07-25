//! Forward commands (spec 02 §6.1): list/create/update/delete/duplicate,
//! reorder, connect/disconnect/retry, global `start_all`/`stop_all`,
//! `get_forward_runtime`, `copy_ssh_command`, set/clear password.
//!
//! Handlers stay thin (AGENTS §3): parse args → mutate `AppState` (RAM) →
//! `persist_*().await?` (F37 surfaces persist failures) → emit the change event
//! → drive the SSH engine. Passwords flow ONLY through `set_forward_password` /
//! `clear_forward_password` — never inside `ForwardInput` (spec 02 §6.1, §8).

use std::sync::Arc;

use tauri::State;
use uuid::Uuid;

use crate::error::AppError;
use crate::ssh::engine;
use crate::state::models::{
    ForwardConfig, ForwardInput, ForwardRuntime, ForwardStatus, TunnelStats,
};
use crate::state::AppState;

/// Build a `ForwardConfig` from an input payload + an id. `has_stored_password`
/// is always seeded from the caller (never derived from the input, which never
/// carries a secret).
fn config_from_input(id: String, input: ForwardInput, has_stored_password: bool) -> ForwardConfig {
    ForwardConfig {
        id,
        name: input.name,
        ssh_host: input.ssh_host,
        ssh_port: input.ssh_port,
        ssh_username: input.ssh_username,
        identity_file_path: input.identity_file_path,
        has_stored_password,
        local_bind_address: input.local_bind_address,
        local_port: input.local_port,
        remote_host: input.remote_host,
        remote_port: input.remote_port,
        keep_alive_interval_sec: input.keep_alive_interval_sec,
        keep_alive_max_count: input.keep_alive_max_count,
        group_id: input.group_id,
        tags: input.tags,
    }
}

/// Build the `ssh -N -L ...` CLI string for a forward (spec 03 §17 — replicate
/// v1 EXACTLY). Token order: `ssh -N -L <fwd> -p <port> [-i <identity>]
/// user@host`. **Never** includes the password (AGENTS §8).
///
/// - `<fwd>` = `<bindPrefix><localPort>:<remoteHost>:<remotePort>` where
///   `bindPrefix` is empty for the default `127.0.0.1` bind, else
///   `"<localBindAddress>:"`.
/// - `-p <sshPort>` is ALWAYS emitted (v1 does, even for port 22).
/// - `-i <identityFilePath>` only when set & non-empty; the path is wrapped in
///   double quotes ONLY if it contains a space (v1's rule).
pub fn build_ssh_command(cfg: &ForwardConfig) -> String {
    let bind_prefix = if cfg.local_bind_address == "127.0.0.1" {
        String::new()
    } else {
        format!("{}:", cfg.local_bind_address)
    };
    let forward_spec = format!(
        "{}{}:{}:{}",
        bind_prefix, cfg.local_port, cfg.remote_host, cfg.remote_port
    );

    let mut parts = vec![
        "ssh".to_string(),
        "-N".to_string(),
        "-L".to_string(),
        forward_spec,
        "-p".to_string(),
        cfg.ssh_port.to_string(),
    ];

    if let Some(identity) = cfg.identity_file_path.as_deref() {
        if !identity.is_empty() {
            parts.push("-i".to_string());
            parts.push(if identity.contains(' ') {
                format!("\"{identity}\"")
            } else {
                identity.to_string()
            });
        }
    }

    parts.push(format!("{}@{}", cfg.ssh_username, cfg.ssh_host));
    parts.join(" ")
}

/// The runtime for an id, or a `Disconnected` default when the tunnel is not
/// live (no registry entry yet).
fn runtime_or_default(state: &Arc<AppState>, id: &str) -> ForwardRuntime {
    state
        .registry
        .runtime(id)
        .unwrap_or_else(|| ForwardRuntime {
            status: ForwardStatus::Disconnected,
            stats: TunnelStats::default(),
            last_error: None,
        })
}

// --- CRUD ---

/// `list_forwards` — display-ordered config list (boot / rehydrate).
#[tauri::command]
pub fn list_forwards(state: State<'_, Arc<AppState>>) -> Vec<ForwardConfig> {
    state.configs_snapshot()
}

/// `create_forward` — append a new forward (fresh uuid, no stored password) and
/// persist. The password (if any) is a separate `set_forward_password` call.
#[tauri::command]
pub async fn create_forward(
    state: State<'_, Arc<AppState>>,
    input: ForwardInput,
) -> Result<ForwardConfig, AppError> {
    let state = state.inner();
    let config = config_from_input(Uuid::new_v4().to_string(), input, false);
    state.upsert_config(config.clone());
    state.persist_forwards().await?;
    state.emit_forwards_changed();
    Ok(config)
}

/// `update_forward` — edit an existing forward. If it is currently live it is
/// force-disconnected first (v1 parity), then the config is replaced preserving
/// its id and `has_stored_password` (the secret is untouched).
#[tauri::command]
pub async fn update_forward(
    state: State<'_, Arc<AppState>>,
    id: String,
    input: ForwardInput,
) -> Result<ForwardConfig, AppError> {
    update_forward_impl(state.inner(), id, input).await
}

/// Testable body of [`update_forward`] over `&Arc<AppState>`.
async fn update_forward_impl(
    state: &Arc<AppState>,
    id: String,
    input: ForwardInput,
) -> Result<ForwardConfig, AppError> {
    let existing = state
        .get_config(&id)
        .ok_or_else(|| AppError::NotFound(format!("forward {id}")))?;

    // Editing a connected/live config force-disconnects first (v1 parity): the
    // running tunnel used the OLD host/port/auth, so it must be torn down before
    // the config changes underneath it.
    if state.registry.contains(&id) {
        engine::disconnect_forward(state, &id, true).await?;
    }

    let config = config_from_input(id, input, existing.has_stored_password);
    state.upsert_config(config.clone());
    state.persist_forwards().await?;
    state.emit_forwards_changed();
    Ok(config)
}

/// `delete_forward` — tear down the tunnel if live, drop its stored secret, then
/// remove the config and persist.
#[tauri::command]
pub async fn delete_forward(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    let state = state.inner();
    if state.registry.contains(&id) {
        engine::disconnect_forward(state, &id, true).await?;
    }
    // Best-effort secret cleanup so a deleted forward never orphans a password.
    state.delete_password(&id);
    if state.remove_config(&id) {
        state.persist_forwards().await?;
        state.emit_forwards_changed();
    }
    Ok(())
}

/// `duplicate_forward` — copy a forward with a new id and " (copy)" suffix. The
/// duplicate never inherits the original's stored password (spec 02 §6.1).
#[tauri::command]
pub async fn duplicate_forward(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<ForwardConfig, AppError> {
    let state = state.inner();
    let source = state
        .get_config(&id)
        .ok_or_else(|| AppError::NotFound(format!("forward {id}")))?;

    let copy = duplicate_config(source);
    state.upsert_config(copy.clone());
    state.persist_forwards().await?;
    state.emit_forwards_changed();
    Ok(copy)
}

/// `reorder_forwards` — set the display order to `ordered_ids`. Any live ids not
/// present in the list are appended in their existing relative order (defensive
/// against a stale/partial reorder request), and unknown ids are ignored.
#[tauri::command]
pub async fn reorder_forwards(
    state: State<'_, Arc<AppState>>,
    ordered_ids: Vec<String>,
) -> Result<(), AppError> {
    let state = state.inner();
    let reordered = reorder_configs(state.configs_snapshot(), &ordered_ids);
    state.replace_configs(reordered);
    state.persist_forwards().await?;
    state.emit_forwards_changed();
    Ok(())
}

/// Duplicate a config with a fresh id, a " (copy)" name suffix, and NO stored
/// password (spec 02 §6.1). Pure so the naming/flag rules are unit-testable.
fn duplicate_config(source: ForwardConfig) -> ForwardConfig {
    ForwardConfig {
        id: Uuid::new_v4().to_string(),
        name: format!("{} (copy)", source.name),
        has_stored_password: false,
        ..source
    }
}

/// Reorder `current` to match `ordered_ids`: known ids first in requested order,
/// then any config the request omitted in its existing relative order (never
/// drop a forward), ignoring ids not in `current`. Pure — unit-testable.
fn reorder_configs(current: Vec<ForwardConfig>, ordered_ids: &[String]) -> Vec<ForwardConfig> {
    let mut reordered: Vec<ForwardConfig> = Vec::with_capacity(current.len());
    for id in ordered_ids {
        if let Some(cfg) = current.iter().find(|c| &c.id == id) {
            reordered.push(cfg.clone());
        }
    }
    for cfg in &current {
        if !ordered_ids.contains(&cfg.id) {
            reordered.push(cfg.clone());
        }
    }
    reordered
}

// --- Connection control (thin wrappers over the engine) ---

/// `connect_forward` — start (or restart) the tunnel's supervisor.
#[tauri::command]
pub async fn connect_forward(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    engine::connect_forward(state.inner(), &id).await
}

/// `disconnect_forward` — user-initiated (silent) disconnect.
#[tauri::command]
pub async fn disconnect_forward(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), AppError> {
    engine::disconnect_forward(state.inner(), &id, true).await
}

/// `retry_forward` — retry a tunnel parked in `error`.
#[tauri::command]
pub async fn retry_forward(state: State<'_, Arc<AppState>>, id: String) -> Result<(), AppError> {
    engine::retry_forward(state.inner(), &id).await
}

/// `get_forward_runtime` — on-demand status+stats+lastError snapshot.
#[tauri::command]
pub fn get_forward_runtime(state: State<'_, Arc<AppState>>, id: String) -> ForwardRuntime {
    runtime_or_default(state.inner(), &id)
}

/// `copy_ssh_command` — the `ssh -N -L ...` string for a forward (no password).
#[tauri::command]
pub fn copy_ssh_command(state: State<'_, Arc<AppState>>, id: String) -> Result<String, AppError> {
    let cfg = state
        .get_config(&id)
        .ok_or_else(|| AppError::NotFound(format!("forward {id}")))?;
    Ok(build_ssh_command(&cfg))
}

// --- Credentials (route through the keychain / fallback store) ---

/// `set_forward_password` — store the secret in the keychain/fallback and flip
/// `has_stored_password`. The password is NEVER logged/emitted (AGENTS §8).
#[tauri::command]
pub async fn set_forward_password(
    state: State<'_, Arc<AppState>>,
    id: String,
    password: String,
) -> Result<(), AppError> {
    let state = state.inner();
    let mut config = state
        .get_config(&id)
        .ok_or_else(|| AppError::NotFound(format!("forward {id}")))?;

    state.set_password_checked(&id, password).await?;
    if !config.has_stored_password {
        config.has_stored_password = true;
        state.upsert_config(config);
        state.persist_forwards().await?;
        state.emit_forwards_changed();
    }
    Ok(())
}

/// `clear_forward_password` — remove the stored secret and clear the flag.
#[tauri::command]
pub async fn clear_forward_password(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), AppError> {
    let state = state.inner();
    let mut config = state
        .get_config(&id)
        .ok_or_else(|| AppError::NotFound(format!("forward {id}")))?;

    state.delete_password_checked(&id).await?;
    if config.has_stored_password {
        config.has_stored_password = false;
        state.upsert_config(config);
        state.persist_forwards().await?;
        state.emit_forwards_changed();
    }
    Ok(())
}

// --- Global bulk (F3) — distinct from per-group start/stop (AGENTS §1) ---

/// Global bulk connect (v1 `connectAll`, F3): connect every configured forward
/// that is not already connected/connecting; retry any parked in `error`.
///
/// Distinct from per-group `start_group` (spec 02 §6.2, AGENTS §1) — this is the
/// global tray/palette/keymap action. Best-effort: a per-tunnel failure is
/// logged inside the engine and does not abort the sweep.
pub async fn run_start_all(state: &Arc<AppState>) -> Result<(), AppError> {
    for cfg in state.configs_snapshot() {
        match state.registry.current_status(&cfg.id) {
            // Already live and healthy/transitioning — leave it.
            Some(ForwardStatus::Connected)
            | Some(ForwardStatus::Connecting)
            | Some(ForwardStatus::Disconnecting) => {}
            // Parked in error — reuse the supervisor via retry.
            Some(ForwardStatus::Error) => {
                if let Err(e) = engine::retry_forward(state, &cfg.id).await {
                    tracing::error!(tunnel = %cfg.id, error = %e, "start_all: retry failed");
                }
            }
            // Disconnected or not live — start a fresh supervisor.
            Some(ForwardStatus::Disconnected) | None => {
                if let Err(e) = engine::connect_forward(state, &cfg.id).await {
                    tracing::error!(tunnel = %cfg.id, error = %e, "start_all: connect failed");
                }
            }
        }
    }
    Ok(())
}

/// Global bulk disconnect (v1 `disconnectAll`, F3): user-initiated (silent)
/// disconnect of every live tunnel. Distinct from per-group `stop_group`.
pub async fn run_stop_all(state: &Arc<AppState>) -> Result<(), AppError> {
    for id in state.registry.all_ids() {
        if let Err(e) = engine::disconnect_forward(state, &id, true).await {
            tracing::error!(tunnel = %id, error = %e, "stop_all: disconnect failed");
        }
    }
    Ok(())
}

/// `start_all` command (spec 02 §6.1) — global bulk connect.
#[tauri::command]
pub async fn start_all(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    run_start_all(state.inner()).await
}

/// `stop_all` command (spec 02 §6.1) — global bulk disconnect (user-initiated).
#[tauri::command]
pub async fn stop_all(state: State<'_, Arc<AppState>>) -> Result<(), AppError> {
    run_stop_all(state.inner()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> ForwardConfig {
        ForwardConfig {
            id: "id".into(),
            name: "Prod DB".into(),
            ssh_host: "bastion.example.com".into(),
            ssh_port: 22,
            ssh_username: "deploy".into(),
            identity_file_path: None,
            has_stored_password: false,
            local_bind_address: "127.0.0.1".into(),
            local_port: 5432,
            remote_host: "db.internal".into(),
            remote_port: 5432,
            keep_alive_interval_sec: 30,
            keep_alive_max_count: 5,
            group_id: None,
            tags: vec![],
        }
    }

    #[test]
    fn default_case_emits_p_22_and_no_bind_prefix() {
        // Default 127.0.0.1 + port 22 + no identity → `-p 22` IS present (v1).
        let cfg = base_config();
        assert_eq!(
            build_ssh_command(&cfg),
            "ssh -N -L 5432:db.internal:5432 -p 22 deploy@bastion.example.com"
        );
    }

    #[test]
    fn non_default_bind_address_adds_prefix() {
        let cfg = ForwardConfig {
            local_bind_address: "0.0.0.0".into(),
            ..base_config()
        };
        assert_eq!(
            build_ssh_command(&cfg),
            "ssh -N -L 0.0.0.0:5432:db.internal:5432 -p 22 deploy@bastion.example.com"
        );
    }

    #[test]
    fn non_default_ssh_port_is_emitted() {
        let cfg = ForwardConfig {
            ssh_port: 2222,
            ..base_config()
        };
        assert!(build_ssh_command(&cfg).contains("-p 2222"));
    }

    #[test]
    fn identity_file_adds_i_flag_unquoted_when_no_space() {
        let cfg = ForwardConfig {
            identity_file_path: Some("/Users/me/.ssh/id_ed25519".into()),
            ..base_config()
        };
        assert_eq!(
            build_ssh_command(&cfg),
            "ssh -N -L 5432:db.internal:5432 -p 22 -i /Users/me/.ssh/id_ed25519 deploy@bastion.example.com"
        );
    }

    #[test]
    fn identity_file_with_space_is_quoted() {
        let cfg = ForwardConfig {
            identity_file_path: Some("/Users/me/My Keys/id".into()),
            ..base_config()
        };
        // Token order: -N, -L, -p, -i, user@host; quoted only because of the space.
        assert_eq!(
            build_ssh_command(&cfg),
            "ssh -N -L 5432:db.internal:5432 -p 22 -i \"/Users/me/My Keys/id\" deploy@bastion.example.com"
        );
    }

    #[test]
    fn empty_identity_path_is_ignored() {
        let cfg = ForwardConfig {
            identity_file_path: Some(String::new()),
            ..base_config()
        };
        assert!(!build_ssh_command(&cfg).contains("-i"));
    }

    #[test]
    fn command_never_contains_a_password() {
        // Passwords are not part of ForwardConfig at all — sanity assert the
        // builder output carries no auth secret material (AGENTS §8).
        let cfg = base_config();
        let out = build_ssh_command(&cfg);
        assert!(!out.to_lowercase().contains("password"));
    }

    #[test]
    fn duplicate_appends_copy_suffix_new_id_and_strips_password() {
        let source = ForwardConfig {
            id: "orig".into(),
            name: "Prod DB".into(),
            has_stored_password: true,
            ..base_config()
        };
        let dup = duplicate_config(source.clone());
        assert_eq!(dup.name, "Prod DB (copy)");
        assert_ne!(dup.id, source.id, "duplicate gets a fresh id");
        assert!(
            !dup.has_stored_password,
            "duplicate never inherits the secret"
        );
        // Everything else is carried over.
        assert_eq!(dup.ssh_host, source.ssh_host);
        assert_eq!(dup.local_port, source.local_port);
    }

    #[test]
    fn reorder_moves_known_ids_and_preserves_omitted() {
        let mk = |id: &str| ForwardConfig {
            id: id.into(),
            ..base_config()
        };
        let current = vec![mk("a"), mk("b"), mk("c")];
        // Request reverses b,c and omits a + names an unknown id.
        let ordered = vec!["c".to_string(), "b".to_string(), "zzz".to_string()];
        let out = reorder_configs(current, &ordered);
        // Requested known ids first (c, b), then the omitted one (a); unknown dropped.
        assert_eq!(
            out.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
            ["c", "b", "a"]
        );
    }

    #[tokio::test]
    async fn update_forward_force_disconnects_a_live_tunnel() {
        use crate::state::tunnel_registry::fake_handle;
        use crate::state::AppState;

        let state = Arc::new(AppState::new_headless());
        let original = ForwardConfig {
            id: "live".into(),
            name: "Before".into(),
            has_stored_password: true,
            ..base_config()
        };
        state.upsert_config(original);
        // Make the tunnel appear live + connected in the registry.
        state
            .registry
            .insert(fake_handle("live", ForwardStatus::Connected));
        assert!(state.registry.contains("live"));

        let input = ForwardInput {
            name: "After".into(),
            ssh_host: "new.example.com".into(),
            ssh_port: 22,
            ssh_username: "deploy".into(),
            identity_file_path: None,
            local_bind_address: "127.0.0.1".into(),
            local_port: 6000,
            remote_host: "db.internal".into(),
            remote_port: 5432,
            keep_alive_interval_sec: 30,
            keep_alive_max_count: 5,
            group_id: None,
            tags: vec![],
        };
        let updated = update_forward_impl(&state, "live".into(), input)
            .await
            .expect("update");

        // Force-disconnected: the registry no longer holds the tunnel.
        assert!(
            !state.registry.contains("live"),
            "editing a connected forward force-disconnects it first (v1 parity)"
        );
        // Config was replaced, id + stored-password flag preserved.
        assert_eq!(updated.id, "live");
        assert_eq!(updated.name, "After");
        assert_eq!(updated.ssh_host, "new.example.com");
        assert!(
            updated.has_stored_password,
            "the stored-password flag is preserved across an edit (secret untouched)"
        );
    }
}
