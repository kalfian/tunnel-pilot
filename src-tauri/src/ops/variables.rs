//! Variable service functions (spec 02 §6.9, 03 §24) — shared by the desktop
//! commands, the web dispatcher and the CLI control socket.
//!
//! A plain variable may appear in a forward's SSH username as `{{NAME}}`; a
//! secret variable may be a forward's password (`passwordVariable`). The roles
//! never mix: a secret is never rendered into a username (which is shown in
//! the UI, the CLI and `copy_ssh_command`), and a password is never stored in
//! the config file. Values resolve when a tunnel dials, so a change applies on
//! the next (re)connect; live tunnels that use it are flagged `needsReconnect`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::AppError;
use crate::state::models::{
    ForwardConfig, ForwardInput, Variable, VariableEndpointInput, VariableInput,
};
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
    // The servers a secret may be sent to survive a value change (whoever
    // changes it knows the new value), but not a switch to plain.
    let endpoints = existing
        .as_ref()
        .filter(|e| e.secret)
        .map(|e| e.allowed_endpoints.clone())
        .unwrap_or_default();
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
            (secret_var(name, true, endpoints), existing.is_some())
        }
        Change::Secret(None) => {
            let changed = existing.as_ref().is_some_and(|e| !e.secret);
            (secret_var(name, was_secret_with_value, endpoints), changed)
        }
        Change::SecretStoredExternally => (secret_var(name, true, endpoints), existing.is_some()),
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
        allowed_endpoints: Vec::new(),
    }
}

fn secret_var(name: &str, has_stored_value: bool, allowed_endpoints: Vec<String>) -> Variable {
    Variable {
        name: name.to_string(),
        secret: true,
        value: String::new(),
        has_stored_value,
        allowed_endpoints,
    }
}

/// The key a secret variable's server binding uses: `host:port`, the host
/// trimmed and ASCII-lowercased (the web UI computes the same).
pub fn endpoint_key(host: &str, port: u16) -> String {
    format!("{}:{}", host.trim().to_ascii_lowercase(), port)
}

/// How a forward save treats a secret variable's server binding (03 §24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    /// The forward's server must already be bound (web saves without proof).
    Required,
    /// Bind the server once the save has succeeded: a local driver (desktop /
    /// CLI, whose user can read the credential store anyway) or a web save
    /// that consumed a proof grant.
    BindAfterSave,
}

/// Bind secret variable `name` to `host:port` for a LOCAL caller. A no-op
/// when already bound. Never reachable from the web transport.
pub(crate) async fn allow_endpoint_trusted(
    state: &Arc<AppState>,
    name: &str,
    host: &str,
    port: u16,
) -> Result<(), AppError> {
    let _guard = state.lock_variables().await;
    add_endpoint(state, name, &endpoint_key(host, port)).await
}

/// How long a proof stays usable for the web action it was given for.
const GRANT_TTL: Duration = Duration::from_secs(120);
/// How long a proof-carrying web save lets an unpinned forward make its first
/// connection from the same web session (that dial pins the server's key).
const PERMIT_TTL: Duration = Duration::from_secs(600);
/// Longest lockout after repeated wrong values.
const MAX_LOCKOUT: Duration = Duration::from_secs(3600);

/// Web-only bookkeeping (spec 03 §24): wrong-value counters per variable and
/// short-lived one-time grants a proof hands to the next web action. Held in
/// `AppState`; RAM only (a restart resets it, like web sessions).
#[derive(Default)]
pub struct WebGuard {
    attempts: Mutex<HashMap<String, Attempts>>,
    grants: Mutex<Vec<Grant>>,
    permits: Mutex<Vec<Permit>>,
}

/// One web session may make the first (key-pinning) dial of forward `id`.
struct Permit {
    scope: String,
    forward_id: String,
    expires: Instant,
}

#[derive(Default)]
struct Attempts {
    failures: u32,
    locked_until: Option<Instant>,
}

/// Grants and permits are scoped to the web session that proved the value,
/// so another session cannot spend them.
struct Grant {
    scope: String,
    name: String,
    endpoint: String,
    expires: Instant,
}

impl WebGuard {
    /// Count an attempt BEFORE the value is compared, with the lockout a wrong
    /// value would earn already in force: a client that hangs up mid-check
    /// cannot skip it. Refused while locked out.
    fn begin_attempt(&self, name: &str) -> Result<(), AppError> {
        let now = Instant::now();
        let mut map = self
            .attempts
            .lock()
            .map_err(|_| AppError::Internal("attempt counter poisoned".into()))?;
        let entry = map.entry(name.to_string()).or_default();
        if let Some(until) = entry.locked_until {
            if until > now {
                let secs = (until - now).as_secs().max(1);
                return Err(AppError::InvalidInput(format!(
                    "too many wrong values for {name}; try again in {secs} s"
                )));
            }
        }
        entry.failures = entry.failures.saturating_add(1);
        let shift = entry.failures.saturating_sub(1).min(12);
        let backoff = Duration::from_secs(1u64 << shift).min(MAX_LOCKOUT);
        entry.locked_until = Some(now + backoff);
        Ok(())
    }

    /// The value matched: forget the provisional failure.
    fn succeed(&self, name: &str) {
        if let Ok(mut map) = self.attempts.lock() {
            map.remove(name);
        }
    }

    /// The value could not be compared (keychain did not answer): the
    /// attempt was not a guess, so take back its provisional failure.
    fn refund(&self, name: &str) {
        if let Ok(mut map) = self.attempts.lock() {
            if let Some(entry) = map.get_mut(name) {
                entry.failures = entry.failures.saturating_sub(1);
                if entry.failures == 0 {
                    map.remove(name);
                }
            }
        }
    }

    fn grant(&self, scope: &str, name: &str, endpoint: String) {
        if let Ok(mut grants) = self.grants.lock() {
            let now = Instant::now();
            grants.retain(|g| g.expires > now);
            grants.push(Grant {
                scope: scope.to_string(),
                name: name.to_string(),
                endpoint,
                expires: now + GRANT_TTL,
            });
        }
    }

    /// Consume this session's live grant for `name` on `endpoint`, if any.
    pub fn take_grant(&self, scope: &str, name: &str, endpoint: &str) -> bool {
        let Ok(mut grants) = self.grants.lock() else {
            return false;
        };
        let now = Instant::now();
        grants.retain(|g| g.expires > now);
        match grants
            .iter()
            .position(|g| g.scope == scope && g.name == name && g.endpoint == endpoint)
        {
            Some(i) => {
                grants.remove(i);
                true
            }
            None => false,
        }
    }

    /// Let this session make the first dial of forward `id` (after a
    /// proof-carrying save of it).
    pub fn permit_first_dial(&self, scope: &str, forward_id: &str) {
        if let Ok(mut permits) = self.permits.lock() {
            let now = Instant::now();
            permits.retain(|p| p.expires > now);
            permits.push(Permit {
                scope: scope.to_string(),
                forward_id: forward_id.to_string(),
                expires: now + PERMIT_TTL,
            });
        }
    }

    /// Whether this session may make the first dial of forward `id`.
    pub fn may_first_dial(&self, scope: &str, forward_id: &str) -> bool {
        let Ok(mut permits) = self.permits.lock() else {
            return false;
        };
        let now = Instant::now();
        permits.retain(|p| p.expires > now);
        permits
            .iter()
            .any(|p| p.scope == scope && p.forward_id == forward_id)
    }

    /// A web session ended: drop what it was given.
    pub fn forget_scope(&self, scope: &str) {
        if let Ok(mut grants) = self.grants.lock() {
            grants.retain(|g| g.scope != scope);
        }
        if let Ok(mut permits) = self.permits.lock() {
            permits.retain(|p| p.scope != scope);
        }
    }
}

/// `allow_variable_endpoint` (web UI): prove a secret variable's value for
/// one server. Success hands out a one-time grant (2 minutes) that the next
/// web action sending the secret there consumes — creating a forward with it,
/// changing a forward's variable or server, trusting or forgetting the host
/// key of a forward that uses it. Wrong values are counted before the check
/// and earn a doubling lockout (1 s … 1 h) per variable, so the value cannot
/// be guessed quickly; no lock is held while checking.
pub async fn allow_endpoint_with_proof(
    state: &Arc<AppState>,
    scope: &str,
    input: VariableEndpointInput,
) -> Result<Variable, AppError> {
    let name = input.name.trim();
    let variable = state
        .get_variable(name)
        .ok_or_else(|| AppError::NotFound(format!("variable {name}")))?;
    if !variable.secret || !variable.has_stored_value {
        return Err(AppError::InvalidInput(format!(
            "{name} is not a secret variable with a value"
        )));
    }
    state.web_guard.begin_attempt(name)?;
    let stored = match state
        .get_password_checked_async(&secret_account(name))
        .await
    {
        Ok(Some(stored)) => stored,
        Ok(None) => {
            state.web_guard.refund(name);
            return Err(AppError::InvalidInput(format!("{name} has no value yet")));
        }
        Err(e) => {
            state.web_guard.refund(name);
            return Err(e);
        }
    };
    if !constant_time_eq(stored.as_bytes(), input.value.as_bytes()) {
        tracing::warn!(variable = %name, "wrong value given for a secret variable");
        return Err(AppError::InvalidInput(format!(
            "that is not the value of {name}"
        )));
    }
    state.web_guard.succeed(name);
    state
        .web_guard
        .grant(scope, name, endpoint_key(&input.ssh_host, input.ssh_port));
    Ok(variable)
}

/// Drop every server binding no forward uses any more, so a binding never
/// outlives the forward that justified it (spec 03 §24). Caller holds the
/// variables lock.
pub(crate) async fn prune_bindings_locked(state: &Arc<AppState>) -> Result<(), AppError> {
    let configs = state.configs_snapshot();
    let mut all = state.variables_snapshot();
    let mut changed = false;
    for v in all.iter_mut().filter(|v| v.secret) {
        let before = v.allowed_endpoints.len();
        v.allowed_endpoints.retain(|e| {
            configs.iter().any(|c| {
                c.password_variable.as_deref() == Some(v.name.as_str())
                    && endpoint_key(&c.ssh_host, c.ssh_port) == *e
            })
        });
        changed |= v.allowed_endpoints.len() != before;
    }
    if changed {
        state.replace_variables(all);
        state.persist_variables().await?;
        state.emit_variables_changed();
    }
    Ok(())
}

/// Whether dialling `cfg` would send a secret variable to a server whose host
/// key is not pinned yet — the dial trusts whatever key answers (spec 03 §24).
pub fn sends_secret_unpinned(state: &AppState, cfg: &ForwardConfig) -> bool {
    cfg.host_key.is_none()
        && cfg
            .password_variable
            .as_deref()
            .and_then(|n| state.get_variable(n))
            .is_some_and(|v| v.secret)
}

/// Bind the saved forward's password variable to its server. Caller holds
/// the variables lock.
pub(crate) async fn bind_saved_locked(
    state: &Arc<AppState>,
    cfg: &ForwardConfig,
) -> Result<(), AppError> {
    if let Some(name) = cfg.password_variable.as_deref() {
        add_endpoint(state, name, &endpoint_key(&cfg.ssh_host, cfg.ssh_port)).await?;
    }
    Ok(())
}

/// `revoke_variable_endpoint`: stop sending secret variable `name` to
/// `endpoint` (a key as listed in `allowedEndpoints`). Removing needs no proof.
pub async fn revoke_endpoint(
    state: &Arc<AppState>,
    name: &str,
    endpoint: &str,
) -> Result<Variable, AppError> {
    let _guard = state.lock_variables().await;
    let mut all = state.variables_snapshot();
    let variable = all
        .iter_mut()
        .find(|v| v.name == name)
        .ok_or_else(|| AppError::NotFound(format!("variable {name}")))?;
    variable.allowed_endpoints.retain(|e| e != endpoint);
    let updated = variable.clone();
    state.replace_variables(all);
    state.persist_variables().await?;
    state.emit_variables_changed();
    tracing::info!(variable = %name, endpoint = %endpoint, "server binding removed");
    Ok(updated)
}

/// Add `key` to a secret variable's allowed servers. Caller holds the lock.
async fn add_endpoint(state: &Arc<AppState>, name: &str, key: &str) -> Result<(), AppError> {
    let mut all = state.variables_snapshot();
    let Some(variable) = all.iter_mut().find(|v| v.name == name && v.secret) else {
        return Err(AppError::InvalidInput(format!(
            "{name} is not a secret variable"
        )));
    };
    if variable.allowed_endpoints.iter().any(|e| e == key) {
        return Ok(());
    }
    variable.allowed_endpoints.push(key.to_string());
    state.replace_variables(all);
    state.persist_variables().await?;
    state.emit_variables_changed();
    tracing::info!(variable = %name, endpoint = %key, "secret variable allowed for a server");
    Ok(())
}

/// Equality whose time does not depend on where the inputs differ (the
/// length difference is folded in rather than returned early).
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let len = a.len().max(b.len());
    let mut diff = u8::from(a.len() != b.len());
    for i in 0..len {
        diff |= a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0);
    }
    diff == 0
}

/// Refused unless secret variable `v` may be sent to `host:port`.
fn check_endpoint(v: &Variable, host: &str, port: u16) -> Result<(), AppError> {
    let key = endpoint_key(host, port);
    if v.allowed_endpoints.contains(&key) {
        Ok(())
    } else {
        Err(AppError::InvalidInput(format!(
            "{} is not allowed for {key} yet: confirm it by entering its value",
            v.name
        )))
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
    input: ForwardInput,
) -> Result<ForwardInput, AppError> {
    check_forward_input_as(state, input, Binding::Required)
}

/// [`check_forward_input`], with the server-binding rule chosen by the caller.
pub fn check_forward_input_as(
    state: &Arc<AppState>,
    mut input: ForwardInput,
    binding: Binding,
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
            Some(v) if binding == Binding::Required => {
                check_endpoint(&v, &input.ssh_host, input.ssh_port)?
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
        // Only a password the forward is recorded to have: never a stray
        // store entry that happens to sit under its id.
        if !cfg.has_stored_password {
            return Ok(None);
        }
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
        // Never send the secret to a server it was not bound to, however the
        // forward got here (spec 03 §24).
        Some(v) if v.secret => check_endpoint(&v, &cfg.ssh_host, cfg.ssh_port)?,
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

    fn input_from(cfg: &ForwardConfig) -> ForwardInput {
        forwards::input_from_config(cfg)
    }

    /// Save-time check as a local driver (desktop / CLI) runs it: the
    /// password variable is bound to the forward's server first.
    async fn local(state: &Arc<AppState>, input: ForwardInput) -> Result<ForwardInput, AppError> {
        if let Some(name) = input.password_variable.as_deref() {
            if state.get_variable(name).is_some_and(|v| v.secret) {
                allow_endpoint_trusted(state, name, &input.ssh_host, input.ssh_port).await?;
            }
        }
        check_forward_input(state, input)
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
        let input = local(&state, forward("{{DB_USER}}", Some("DB_PASS")))
            .await
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
        let input = local(&state, forward("{{USER}}", Some("PASS")))
            .await
            .expect("ok");
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
        let input = local(&state, forward("{{USER}}", None)).await.expect("ok");
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
        let input = local(&state, forward("u", Some("PASS"))).await.expect("ok");
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
        let input = local(&state, forward("{{USER}}", None)).await.expect("ok");
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
        let input = local(&state, forward("u", Some("PASS"))).await.expect("ok");
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
        let input = local(&state, forward("u", Some("PASS"))).await.expect("ok");
        let cfg = forwards::create(&state, input).await.expect("create");
        let err = resolve_password(&state, &cfg).await.expect_err("no value");
        assert!(matches!(err, AppError::InvalidInput(_)));
    }

    #[tokio::test]
    async fn a_secret_is_only_sent_to_servers_it_was_bound_to() {
        let state = state();
        save(&state, secret_input("PASS", Some("s3cret")))
            .await
            .expect("secret");
        // A save that requires a binding is refused for an unbound server...
        let err = check_forward_input(&state, forward("u", Some("PASS"))).expect_err("unbound");
        assert!(err.to_string().contains("bastion:22"));
        // ...a local driver binds the server after the save succeeds...
        let cfg = forwards::create_as(&state, forward("u", Some("PASS")), Binding::BindAfterSave)
            .await
            .expect("local create");
        assert_eq!(
            state.get_variable("PASS").expect("var").allowed_endpoints,
            vec!["bastion:22".to_string()]
        );
        // ...re-aimed elsewhere behind the app's back, the secret is not sent.
        let mut moved = cfg.clone();
        moved.ssh_host = "evil.example".into();
        let err = resolve_password(&state, &moved)
            .await
            .expect_err("unbound server");
        assert!(err.to_string().contains("evil.example:22"));

        // Revoking a server stops it too; deleting the last user prunes it.
        revoke_endpoint(&state, "PASS", "bastion:22")
            .await
            .expect("revoke");
        assert!(resolve_password(&state, &cfg).await.is_err());
        forwards::create_as(&state, forward("u", Some("PASS")), Binding::BindAfterSave)
            .await
            .expect("rebind");
        forwards::delete(&state, &cfg.id).await.expect("delete one");
        assert_eq!(
            state
                .get_variable("PASS")
                .expect("var")
                .allowed_endpoints
                .len(),
            1,
            "still used by the second forward"
        );
    }

    #[tokio::test]
    async fn bindings_never_outlive_the_forwards_that_use_them() {
        let state = state();
        save(&state, secret_input("PASS", Some("s3cret")))
            .await
            .expect("secret");
        let cfg = forwards::create_as(&state, forward("u", Some("PASS")), Binding::BindAfterSave)
            .await
            .expect("create");
        // Moving the forward drops the old server's binding and binds the new one.
        let mut moved = input_from(&cfg);
        moved.ssh_host = "other".into();
        forwards::update_as(&state, cfg.id.clone(), moved, Binding::BindAfterSave)
            .await
            .expect("move");
        assert_eq!(
            state.get_variable("PASS").expect("var").allowed_endpoints,
            vec!["other:22".to_string()]
        );
        // A failed local save binds nothing.
        let mut both = forward("u", Some("PASS"));
        both.ssh_host = "never".into();
        both.identity_file_path = Some("/k".into());
        assert!(forwards::create_as(&state, both, Binding::BindAfterSave)
            .await
            .is_err());
        assert!(!state
            .get_variable("PASS")
            .expect("var")
            .allowed_endpoints
            .contains(&"never:22".to_string()));
        forwards::delete(&state, &cfg.id).await.expect("delete");
        assert!(state
            .get_variable("PASS")
            .expect("var")
            .allowed_endpoints
            .is_empty());
    }

    #[tokio::test]
    async fn wrong_values_are_counted_before_the_check_and_lock_out() {
        let state = state();
        save(&state, secret_input("PASS", Some("s3cret")))
            .await
            .expect("secret");
        let proof = |value: &str| VariableEndpointInput {
            name: "PASS".into(),
            ssh_host: "Bastion ".into(),
            ssh_port: 22,
            value: value.into(),
        };
        assert!(allow_endpoint_with_proof(&state, "s1", proof("guess"))
            .await
            .is_err());
        // Locked out now: even the right value is refused without a check.
        let err = allow_endpoint_with_proof(&state, "s1", proof("s3cret"))
            .await
            .expect_err("locked");
        assert!(err.to_string().contains("too many wrong values"));
        // A cancelled attempt cannot dodge the count: the lockout is set first.
        state.web_guard.begin_attempt("OTHER").expect("first");
        assert!(state.web_guard.begin_attempt("OTHER").is_err());

        tokio::time::sleep(Duration::from_millis(1100)).await;
        allow_endpoint_with_proof(&state, "s1", proof("s3cret"))
            .await
            .expect("proof");
        // The grant is for one action on that exact server (host normalised).
        assert!(!state.web_guard.take_grant("s1", "PASS", "evil:22"));
        assert!(
            !state.web_guard.take_grant("s2", "PASS", "bastion:22"),
            "another session cannot spend it"
        );
        assert!(state.web_guard.take_grant("s1", "PASS", "bastion:22"));
        assert!(
            !state.web_guard.take_grant("s1", "PASS", "bastion:22"),
            "one-time"
        );
        // Proving binds nothing by itself.
        assert!(state
            .get_variable("PASS")
            .expect("var")
            .allowed_endpoints
            .is_empty());
    }

    #[test]
    fn constant_time_eq_compares_whole_inputs() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(constant_time_eq(b"", b""));
    }

    #[tokio::test]
    async fn bindings_survive_a_value_change_but_not_a_switch_to_plain() {
        let state = state();
        save(&state, secret_input("PASS", Some("a")))
            .await
            .expect("secret");
        allow_endpoint_trusted(&state, "PASS", "h", 22)
            .await
            .expect("bind");
        let v = save(&state, secret_input("PASS", Some("b")))
            .await
            .expect("rotate");
        assert_eq!(v.allowed_endpoints, vec!["h:22".to_string()]);
        let v = save(&state, plain_input("PASS", "visible"))
            .await
            .expect("plain");
        assert!(v.allowed_endpoints.is_empty());
    }

    #[tokio::test]
    async fn only_unpinned_secret_dials_are_flagged() {
        let state = state();
        save(&state, secret_input("PASS", Some("pw")))
            .await
            .expect("secret");
        save(&state, plain_input("USER", "u")).await.expect("plain");
        let mut cfg = forwards::create_as(
            &state,
            forward("{{USER}}", Some("PASS")),
            Binding::BindAfterSave,
        )
        .await
        .expect("create");
        assert!(sends_secret_unpinned(&state, &cfg));
        cfg.host_key = Some(crate::state::models::HostKeyPin {
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:x".into(),
            pinned_at: None,
        });
        assert!(!sends_secret_unpinned(&state, &cfg), "pinned");
        cfg.host_key = None;
        cfg.password_variable = None;
        assert!(!sends_secret_unpinned(&state, &cfg), "no secret variable");
    }
}
