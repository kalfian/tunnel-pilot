//! Variable service functions (spec 02 §6.9, 03 §24) — shared by the desktop
//! commands, the web dispatcher and the CLI control socket.
//!
//! A plain variable may appear in a forward's SSH username as `{{NAME}}`; a
//! secret variable may be a forward's password (`passwordVariable`). The roles
//! never mix: a secret is never rendered into a username (which is shown in
//! the UI, the CLI and `copy_ssh_command`), and a password is never stored in
//! the config file. Values resolve when a tunnel dials, so a change applies on
//! the next (re)connect; live tunnels that use it are flagged `needsReconnect`.

use std::sync::Arc;

use crate::error::AppError;
use crate::state::models::{ForwardConfig, ForwardInput, Variable, VariableInput};
use crate::state::AppState;
use crate::variables::{
    referenced_names, secret_account, validate_name, validate_plain_value, validate_secret_value,
};

/// The variables, in display order. Never carries a secret value.
pub fn list(state: &Arc<AppState>) -> Vec<Variable> {
    state.variables_snapshot()
}

/// What a save does to a variable's value.
enum Change {
    /// A plain value.
    Plain(String),
    /// A secret: store this new value, or keep the stored one (`None`).
    Secret(Option<String>),
    /// A secret the CLI process already wrote into the credential store itself,
    /// so the value never crosses the control socket (spec 03 §20).
    SecretStoredExternally,
}

/// `save_variable` — create a variable or change its value / kind. Names are
/// unique ignoring case and cannot be renamed (forwards reference them).
pub async fn save(state: &Arc<AppState>, input: VariableInput) -> Result<Variable, AppError> {
    let change = if input.secret {
        Change::Secret(input.value)
    } else {
        Change::Plain(input.value.unwrap_or_default().trim().to_string())
    };
    apply(state, input.name.trim(), change).await
}

/// The CLI wrote secret variable `name`'s value into the credential store
/// from its own process; record that (create the variable if needed).
pub async fn mark_secret_stored(state: &Arc<AppState>, name: &str) -> Result<Variable, AppError> {
    apply(state, name.trim(), Change::SecretStoredExternally).await
}

async fn apply(state: &Arc<AppState>, name: &str, change: Change) -> Result<Variable, AppError> {
    validate_name(name)?;
    let _guard = state.lock_variables().await;
    let existing = state
        .variables_snapshot()
        .into_iter()
        .find(|v| v.name.eq_ignore_ascii_case(name));
    if let Some(e) = &existing {
        if e.name != name {
            return Err(AppError::InvalidInput(format!(
                "a variable named {} already exists (names ignore case)",
                e.name
            )));
        }
    }
    let secret = !matches!(change, Change::Plain(_));
    check_role(state, name, secret)?;

    let was_secret_with_value = existing
        .as_ref()
        .is_some_and(|e| e.secret && e.has_stored_value);
    let (variable, value_changed) = match change {
        Change::Plain(value) => {
            validate_plain_value(name, &value)?;
            if was_secret_with_value {
                state.delete_password_checked(&secret_account(name)).await?;
            }
            let changed = existing
                .as_ref()
                .is_some_and(|e| e.secret || e.value != value);
            (plain(name, value), changed)
        }
        Change::Secret(Some(value)) => {
            validate_secret_value(name, &value)?;
            state
                .set_password_checked(&secret_account(name), value)
                .await?;
            (secret_var(name, true), existing.is_some())
        }
        Change::Secret(None) => {
            let changed = existing.as_ref().is_some_and(|e| !e.secret);
            (secret_var(name, was_secret_with_value), changed)
        }
        Change::SecretStoredExternally => (secret_var(name, true), existing.is_some()),
    };

    let mut all = state.variables_snapshot();
    match all.iter_mut().find(|v| v.name == name) {
        Some(slot) => *slot = variable.clone(),
        None => all.push(variable.clone()),
    }
    state.replace_variables(all);
    state.persist_variables().await?;
    state.emit_variables_changed();
    if value_changed {
        flag_live_users(state, name);
    }
    tracing::info!(variable = %name, secret, "variable saved");
    Ok(variable)
}

fn plain(name: &str, value: String) -> Variable {
    Variable {
        name: name.to_string(),
        secret: false,
        value,
        has_stored_value: false,
    }
}

fn secret_var(name: &str, has_stored_value: bool) -> Variable {
    Variable {
        name: name.to_string(),
        secret: true,
        value: String::new(),
        has_stored_value,
    }
}

/// A variable cannot change into a kind its forwards cannot use: a secret in a
/// username would be displayed; a plain value as a password would sit in the
/// config file.
fn check_role(state: &Arc<AppState>, name: &str, secret: bool) -> Result<(), AppError> {
    let configs = state.configs_snapshot();
    let users: Vec<&str> = configs
        .iter()
        .filter(|c| {
            if secret {
                crate::variables::references(&c.ssh_username, name)
            } else {
                c.password_variable.as_deref() == Some(name)
            }
        })
        .map(|c| c.name.as_str())
        .collect();
    if users.is_empty() {
        return Ok(());
    }
    let list = users.join(", ");
    Err(AppError::InvalidInput(if secret {
        format!("{name} is used in the SSH username of {list}; a secret variable can only be a password")
    } else {
        format!("{name} is the password of {list}; it must stay secret")
    }))
}

/// `delete_variable` — refused while any forward uses it (the forward would
/// silently stop authenticating); its secret is removed with it.
pub async fn delete(state: &Arc<AppState>, name: &str) -> Result<(), AppError> {
    let _guard = state.lock_variables().await;
    let Some(variable) = state.get_variable(name) else {
        return Err(AppError::NotFound(format!("variable {name}")));
    };
    let users: Vec<String> = state
        .configs_snapshot()
        .into_iter()
        .filter(|c| c.uses_variable(name))
        .map(|c| c.name)
        .collect();
    if !users.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "{name} is used by {}; change those forwards first",
            users.join(", ")
        )));
    }
    if variable.secret {
        state.delete_password_checked(&secret_account(name)).await?;
    }
    let mut all = state.variables_snapshot();
    all.retain(|v| v.name != name);
    state.replace_variables(all);
    state.persist_variables().await?;
    state.emit_variables_changed();
    tracing::info!(variable = %name, "variable deleted");
    Ok(())
}

/// Record the change for every live tunnel that uses `name`, so a connected one
/// offers a reconnect (and one mid-dial is judged when it connects).
fn flag_live_users(state: &Arc<AppState>, name: &str) {
    let gen = state.registry.bump_variables_gen();
    for cfg in state.configs_snapshot() {
        if cfg.uses_variable(name) && state.registry.note_variable_changed(&cfg.id, gen) {
            if let Some(rt) = state.registry.runtime(&cfg.id) {
                state.emit_status(&cfg.id, rt.status, rt.last_error);
            }
        }
    }
}

/// Check a forward's variable references before it is saved, and normalise
/// them: a blank `passwordVariable` is `None`. Every referenced variable must
/// exist with the right kind, so a typo fails here rather than at dial time.
pub fn check_forward_input(
    state: &Arc<AppState>,
    mut input: ForwardInput,
) -> Result<ForwardInput, AppError> {
    for name in referenced_names(&input.ssh_username)? {
        match state.get_variable(&name) {
            None => {
                return Err(AppError::InvalidInput(format!(
                    "the SSH username uses {{{{{name}}}}}, which is not a variable; create it first"
                )))
            }
            Some(v) if v.secret => {
                return Err(AppError::InvalidInput(format!(
                    "{name} is secret; a username can only use plain variables"
                )))
            }
            Some(_) => {}
        }
    }
    input.password_variable = input
        .password_variable
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty());
    if let Some(name) = &input.password_variable {
        match state.get_variable(name) {
            None => {
                return Err(AppError::InvalidInput(format!(
                    "password variable {name} does not exist; create it first"
                )))
            }
            Some(v) if !v.secret => {
                return Err(AppError::InvalidInput(format!(
                    "{name} is not secret; only a secret variable can be a password"
                )))
            }
            Some(_) => {}
        }
        if input
            .identity_file_path
            .as_deref()
            .is_some_and(|p| !p.trim().is_empty())
        {
            return Err(AppError::InvalidInput(
                "use either an identity file or a password variable, not both".into(),
            ));
        }
    }
    Ok(input)
}

/// The SSH username a forward logs in as now: its template with every
/// `{{NAME}}` replaced by the plain variable's current value.
pub fn resolve_username(state: &AppState, cfg: &ForwardConfig) -> Result<String, AppError> {
    let mut secret_used = None;
    let rendered = crate::variables::render(&cfg.ssh_username, |name| {
        let v = state.get_variable(name)?;
        if v.secret {
            secret_used = Some(name.to_string());
            return Some(String::new());
        }
        Some(v.value)
    });
    if let Some(name) = secret_used {
        return Err(AppError::InvalidInput(format!(
            "{name} is secret; a username can only use plain variables"
        )));
    }
    let username = rendered?;
    if username.trim().is_empty() {
        return Err(AppError::InvalidInput("the SSH username is empty".into()));
    }
    Ok(username)
}

/// The password to authenticate with, read at dial time: the secret variable's
/// value when the forward names one, else the forward's own stored password.
/// `Ok(None)` = no password anywhere (the caller reports it).
pub async fn resolve_password(
    state: &AppState,
    cfg: &ForwardConfig,
) -> Result<Option<String>, AppError> {
    let Some(name) = cfg.password_variable.as_deref() else {
        return Ok(state.get_password_async(&cfg.id).await);
    };
    let no_value = || {
        AppError::InvalidInput(format!(
            "password variable {name} has no value yet (Settings → Variables, or `var set {name} --secret`)"
        ))
    };
    match state.get_variable(name) {
        // Trust the recorded state, not a leftover `var:NAME` entry (e.g.
        // from an earlier install) that the user has never seen.
        Some(v) if v.secret && !v.has_stored_value => return Err(no_value()),
        Some(v) if v.secret => {}
        Some(_) => {
            return Err(AppError::InvalidInput(format!(
                "password variable {name} is not secret"
            )))
        }
        None => {
            return Err(AppError::InvalidInput(format!(
                "password variable {name} does not exist (Settings → Variables, or `var set {name} --secret`)"
            )))
        }
    }
    // A failed read (keychain locked, D-Bus timeout after wake) is a
    // `credential` error and retried like any transient failure; only a value
    // that is really absent is the user's to fix (`invalidInput`, not retried).
    match state
        .get_password_checked_async(&secret_account(name))
        .await
    {
        Ok(Some(pw)) => Ok(Some(pw)),
        Ok(None) => Err(no_value()),
        Err(e) => {
            tracing::warn!(variable = %name, error = %e, "could not read a secret variable");
            Err(AppError::Credential(format!(
                "could not read password variable {name} from the keychain"
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::forwards;
    use crate::state::models::ForwardStatus;
    use crate::state::tunnel_registry::fake_handle;

    fn state() -> Arc<AppState> {
        Arc::new(AppState::new_headless())
    }

    fn plain_input(name: &str, value: &str) -> VariableInput {
        VariableInput {
            name: name.into(),
            secret: false,
            value: Some(value.into()),
        }
    }

    fn secret_input(name: &str, value: Option<&str>) -> VariableInput {
        VariableInput {
            name: name.into(),
            secret: true,
            value: value.map(str::to_string),
        }
    }

    fn forward(username: &str, password_variable: Option<&str>) -> ForwardInput {
        ForwardInput {
            name: "db".into(),
            ssh_host: "bastion".into(),
            ssh_port: 22,
            ssh_username: username.into(),
            identity_file_path: None,
            local_bind_address: "127.0.0.1".into(),
            local_port: 15432,
            remote_host: "db".into(),
            remote_port: 5432,
            keep_alive_interval_sec: 30,
            keep_alive_max_count: 5,
            group_id: None,
            tags: vec![],
            password_variable: password_variable.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn plain_and_secret_variables_resolve_at_dial_time() {
        let state = state();
        save(&state, plain_input("DB_USER", "kalfian"))
            .await
            .expect("user");
        save(&state, secret_input("DB_PASS", Some("hunter2")))
            .await
            .expect("pass");
        let input = check_forward_input(&state, forward("{{DB_USER}}", Some("DB_PASS")))
            .expect("valid refs");
        let cfg = forwards::create(&state, input).await.expect("create");

        assert_eq!(resolve_username(&state, &cfg).expect("user"), "kalfian");
        assert_eq!(
            resolve_password(&state, &cfg).await.expect("pw").as_deref(),
            Some("hunter2")
        );
        assert!(!cfg.needs_password());

        // Changing the values is picked up by the next dial, with no edit.
        save(&state, plain_input("DB_USER", "ops"))
            .await
            .expect("user2");
        save(&state, secret_input("DB_PASS", Some("n3w")))
            .await
            .expect("pass2");
        assert_eq!(resolve_username(&state, &cfg).expect("user"), "ops");
        assert_eq!(
            resolve_password(&state, &cfg).await.expect("pw").as_deref(),
            Some("n3w")
        );
    }

    #[tokio::test]
    async fn secret_values_never_appear_in_lists_or_snapshots() {
        let state = state();
        let saved = save(&state, secret_input("DB_PASS", Some("s3cr3t-value")))
            .await
            .expect("save");
        assert!(saved.has_stored_value && saved.value.is_empty());
        let listed = serde_json::to_string(&list(&state)).expect("json");
        assert!(!listed.contains("s3cr3t-value"));
        let snap = serde_json::to_string(&state.app_snapshot(Default::default())).expect("snap");
        assert!(!snap.contains("s3cr3t-value"));

        // Keeping the stored secret: a save without a value leaves it alone.
        let kept = save(&state, secret_input("DB_PASS", None))
            .await
            .expect("keep");
        assert!(kept.has_stored_value);
        assert_eq!(
            state.get_password(&secret_account("DB_PASS")).as_deref(),
            Some("s3cr3t-value")
        );
    }

    #[tokio::test]
    async fn references_are_checked_on_save() {
        let state = state();
        assert!(check_forward_input(&state, forward("{{NOPE}}", None)).is_err());
        assert!(check_forward_input(&state, forward("u", Some("NOPE"))).is_err());

        save(&state, plain_input("USER", "u")).await.expect("plain");
        save(&state, secret_input("PASS", None))
            .await
            .expect("secret");
        // Roles never mix.
        assert!(check_forward_input(&state, forward("{{PASS}}", None)).is_err());
        assert!(check_forward_input(&state, forward("u", Some("USER"))).is_err());
        // A password variable excludes an identity file.
        let mut both = forward("u", Some("PASS"));
        both.identity_file_path = Some("/k".into());
        assert!(check_forward_input(&state, both).is_err());
        // Blank means none.
        let blank = check_forward_input(&state, forward("{{USER}}", Some("  "))).expect("ok");
        assert_eq!(blank.password_variable, None);
    }

    #[tokio::test]
    async fn names_are_unique_ignoring_case() {
        let state = state();
        save(&state, plain_input("DB_USER", "a"))
            .await
            .expect("first");
        assert!(save(&state, plain_input("db_user", "b")).await.is_err());
        assert!(save(&state, plain_input("bad-name", "b")).await.is_err());
        assert_eq!(list(&state).len(), 1);
    }

    #[tokio::test]
    async fn variables_in_use_cannot_be_deleted_or_change_kind() {
        let state = state();
        save(&state, plain_input("USER", "u")).await.expect("plain");
        save(&state, secret_input("PASS", Some("pw")))
            .await
            .expect("secret");
        let input = check_forward_input(&state, forward("{{USER}}", Some("PASS"))).expect("ok");
        let cfg = forwards::create(&state, input).await.expect("create");

        assert!(delete(&state, "USER").await.is_err());
        assert!(delete(&state, "PASS").await.is_err());
        assert!(save(&state, secret_input("USER", Some("x"))).await.is_err());
        assert!(save(&state, plain_input("PASS", "x")).await.is_err());

        forwards::delete(&state, &cfg.id)
            .await
            .expect("delete forward");
        delete(&state, "PASS").await.expect("now free");
        assert_eq!(
            state.get_password(&secret_account("PASS")),
            None,
            "secret removed"
        );
        assert!(matches!(
            delete(&state, "PASS").await,
            Err(AppError::NotFound(_))
        ));
    }

    #[tokio::test]
    async fn a_secret_turned_plain_drops_its_stored_value() {
        let state = state();
        save(&state, secret_input("X", Some("pw")))
            .await
            .expect("secret");
        let v = save(&state, plain_input("X", "visible"))
            .await
            .expect("plain");
        assert!(!v.secret && v.value == "visible");
        assert_eq!(state.get_password(&secret_account("X")), None);
    }

    #[tokio::test]
    async fn changing_a_used_variable_flags_live_tunnels_only() {
        let state = state();
        save(&state, plain_input("USER", "u")).await.expect("plain");
        let input = check_forward_input(&state, forward("{{USER}}", None)).expect("ok");
        let live = forwards::create(&state, input.clone()).await.expect("live");
        let parked = forwards::create(
            &state,
            ForwardInput {
                name: "b".into(),
                ..input
            },
        )
        .await
        .expect("parked");
        state
            .registry
            .insert(fake_handle(&live.id, ForwardStatus::Connected));
        state
            .registry
            .insert(fake_handle(&parked.id, ForwardStatus::Error));

        // Re-saving the same value is not a change.
        save(&state, plain_input("USER", "u")).await.expect("same");
        assert!(!state.registry.needs_reconnect(&live.id));

        save(&state, plain_input("USER", "v"))
            .await
            .expect("changed");
        assert!(state.registry.needs_reconnect(&live.id));
        assert!(!state.registry.needs_reconnect(&parked.id));
        assert!(
            forwards::runtime_or_default(&state, &live.id).needs_reconnect,
            "exposed on the runtime"
        );
    }

    #[tokio::test]
    async fn missing_values_fail_with_a_clear_reason() {
        let state = state();
        save(&state, secret_input("PASS", None))
            .await
            .expect("no value yet");
        let input = check_forward_input(&state, forward("u", Some("PASS"))).expect("ok");
        let cfg = forwards::create(&state, input).await.expect("create");
        let err = resolve_password(&state, &cfg).await.expect_err("no value");
        assert!(err.to_string().contains("has no value"));

        let mut orphan = cfg.clone();
        orphan.ssh_username = "{{GONE}}".into();
        assert!(resolve_username(&state, &orphan)
            .expect_err("undefined")
            .to_string()
            .contains("GONE"));
    }

    #[tokio::test]
    async fn a_change_during_a_dial_is_judged_by_what_the_dial_resolved() {
        let state = state();
        save(&state, plain_input("USER", "u")).await.expect("plain");
        let input = check_forward_input(&state, forward("{{USER}}", None)).expect("ok");
        let early = forwards::create(&state, input.clone())
            .await
            .expect("early");
        let late = forwards::create(
            &state,
            ForwardInput {
                name: "late".into(),
                ..input
            },
        )
        .await
        .expect("late");
        for id in [&early.id, &late.id] {
            state
                .registry
                .insert(fake_handle(id, ForwardStatus::Connecting));
        }
        // `early` resolved its values before the change, `late` after it.
        state
            .registry
            .record_variables_resolved(&early.id, state.registry.variables_gen());
        save(&state, plain_input("USER", "v"))
            .await
            .expect("change");
        state
            .registry
            .record_variables_resolved(&late.id, state.registry.variables_gen());
        assert!(
            !state.registry.needs_reconnect(&early.id),
            "not while dialling"
        );

        for id in [&early.id, &late.id] {
            state
                .registry
                .set_status(id, ForwardStatus::Connected, None);
        }
        assert!(
            state.registry.needs_reconnect(&early.id),
            "dialled on the old value"
        );
        assert!(
            !state.registry.needs_reconnect(&late.id),
            "already has the new value"
        );
    }

    #[tokio::test]
    async fn storing_an_own_password_switches_off_the_password_variable() {
        let state = state();
        save(&state, secret_input("PASS", Some("shared")))
            .await
            .expect("secret");
        let input = check_forward_input(&state, forward("u", Some("PASS"))).expect("ok");
        let cfg = forwards::create(&state, input).await.expect("create");
        forwards::set_password(&state, &cfg.id, "own".into())
            .await
            .expect("own password");
        let cfg = state.get_config(&cfg.id).expect("cfg");
        assert_eq!(cfg.password_variable, None);
        assert!(cfg.has_stored_password);
        assert_eq!(
            resolve_password(&state, &cfg).await.expect("pw").as_deref(),
            Some("own")
        );
    }

    #[tokio::test]
    async fn a_leftover_store_entry_is_not_used_for_a_variable_without_a_value() {
        let state = state();
        // e.g. left behind by an earlier install
        state
            .set_password_checked(&secret_account("PASS"), "stale".into())
            .await
            .expect("seed");
        save(&state, secret_input("PASS", None))
            .await
            .expect("no value");
        let input = check_forward_input(&state, forward("u", Some("PASS"))).expect("ok");
        let cfg = forwards::create(&state, input).await.expect("create");
        let err = resolve_password(&state, &cfg).await.expect_err("no value");
        assert!(matches!(err, AppError::InvalidInput(_)));
    }
}
