//! Wire protocol for the control socket: one JSON object per line, both
//! directions (NDJSON). Every struct is `camelCase` on the wire, matching the
//! rest of the app's serde conventions (AGENTS §1).
//!
//! The request/response pair is deliberately tiny and versioned: a client that
//! speaks a different `v` is rejected outright rather than half-understood.
//! Errors are the app's own [`AppError`], serialized verbatim
//! (`{"kind":"notFound","message":"…"}`), so the CLI can map `kind` → exit code
//! without a second error vocabulary.
//!
//! **No field here ever carries a secret** (AGENTS §8): [`ForwardView`] exposes
//! `hasStoredPassword` (a boolean) and there is no password command.

use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::models::{ForwardConfig, ForwardRuntime, ForwardStatus, TunnelStats};

/// Wire protocol version. Bumped only on a breaking schema change; the server
/// rejects any other value with `invalidInput`.
pub const PROTOCOL_VERSION: u32 = 1;

/// Default wait budget for `connect`/`connect-all` when the client sends none.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;

/// Server-side clamp on `timeoutMs` — a client must not be able to pin a
/// server task open indefinitely.
pub const MIN_TIMEOUT_MS: u64 = 1_000;
/// Upper clamp on `timeoutMs` (5 minutes).
pub const MAX_TIMEOUT_MS: u64 = 300_000;

/// One request line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliRequest {
    /// Protocol version; must equal [`PROTOCOL_VERSION`].
    pub v: u32,
    pub cmd: CliCommand,
    /// Target id or name — required by `status`/`connect`/`disconnect`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Wait budget for `connect`/`connect-all`; clamped server-side to
    /// [`MIN_TIMEOUT_MS`]..=[`MAX_TIMEOUT_MS`], defaulted to
    /// [`DEFAULT_TIMEOUT_MS`] when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    /// Whether to await a terminal status before responding (`--no-wait` → false).
    #[serde(default)]
    pub wait: bool,
}

impl CliRequest {
    /// A request at the current protocol version.
    pub fn new(cmd: CliCommand) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            cmd,
            target: None,
            timeout_ms: None,
            wait: false,
        }
    }

    /// The effective wait budget in ms: caller value clamped into
    /// [`MIN_TIMEOUT_MS`]..=[`MAX_TIMEOUT_MS`], or the default when absent.
    pub fn effective_timeout_ms(&self) -> u64 {
        self.timeout_ms
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS)
    }
}

/// The command surface exposed over the socket. Deliberately read + connection
/// control only: no CRUD, no credentials, no settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CliCommand {
    List,
    Status,
    Connect,
    Disconnect,
    ConnectAll,
    DisconnectAll,
    Version,
}

/// One response line. Exactly one of `data` / `error` is present.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliResponse {
    pub v: u32,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<CliData>,
    /// The app's own error, serialized verbatim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AppError>,
}

impl CliResponse {
    pub fn ok(data: CliData) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(error: AppError) -> Self {
        Self {
            v: PROTOCOL_VERSION,
            ok: false,
            data: None,
            error: Some(error),
        }
    }
}

/// Response payloads. `untagged` so `--json` prints the bare payload an agent
/// wants (`{"forwards":[…]}`) rather than a wrapper. Each variant's required
/// field set is disjoint, which keeps the untagged deserialization
/// unambiguous — keep it that way when adding variants.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", untagged)]
pub enum CliData {
    Status {
        forward: ForwardView,
        stats: TunnelStats,
    },
    Action {
        forward: ForwardView,
        /// Whether the server awaited a terminal status.
        waited: bool,
        /// Whether the wait budget elapsed first (status is non-terminal).
        timed_out: bool,
    },
    Bulk {
        results: Vec<BulkResult>,
        succeeded: usize,
        failed: usize,
    },
    List {
        forwards: Vec<ForwardView>,
    },
    Version {
        app_version: String,
        protocol: u32,
    },
}

/// A forward as the CLI sees it: config + live runtime, flattened. Contains no
/// secret — `has_stored_password` is a flag only (AGENTS §8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardView {
    pub id: String,
    pub name: String,
    pub local_bind_address: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub ssh_username: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub status: ForwardStatus,
    pub last_error: Option<String>,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
    pub has_stored_password: bool,
}

impl ForwardView {
    /// Pure mapping from the persisted config + the live runtime.
    pub fn from_parts(cfg: &ForwardConfig, runtime: &ForwardRuntime) -> Self {
        Self {
            id: cfg.id.clone(),
            name: cfg.name.clone(),
            local_bind_address: cfg.local_bind_address.clone(),
            local_port: cfg.local_port,
            remote_host: cfg.remote_host.clone(),
            remote_port: cfg.remote_port,
            ssh_username: cfg.ssh_username.clone(),
            ssh_host: cfg.ssh_host.clone(),
            ssh_port: cfg.ssh_port,
            status: runtime.status,
            last_error: runtime.last_error.clone(),
            group_id: cfg.group_id.clone(),
            tags: cfg.tags.clone(),
            has_stored_password: cfg.has_stored_password,
        }
    }

    /// `local_bind_address:local_port` — the address a user actually dials.
    pub fn local_endpoint(&self) -> String {
        format!("{}:{}", self.local_bind_address, self.local_port)
    }

    /// `remote_host:remote_port` as seen from the SSH host.
    pub fn remote_endpoint(&self) -> String {
        format!("{}:{}", self.remote_host, self.remote_port)
    }
}

/// Per-tunnel outcome inside a bulk (`connect-all` / `disconnect-all`) result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkResult {
    pub id: String,
    pub name: String,
    pub status: ForwardStatus,
    pub last_error: Option<String>,
    /// The wait budget elapsed before this tunnel reached a terminal status.
    pub timed_out: bool,
}

/// The wire label for a status, matching the serde representation used by the
/// frontend (`connected`, `disconnected`, …).
pub fn status_label(status: ForwardStatus) -> &'static str {
    match status {
        ForwardStatus::Disconnected => "disconnected",
        ForwardStatus::Connecting => "connecting",
        ForwardStatus::Connected => "connected",
        ForwardStatus::Disconnecting => "disconnecting",
        ForwardStatus::Error => "error",
    }
}

#[cfg(test)]
pub(crate) fn sample_config(id: &str, name: &str) -> ForwardConfig {
    ForwardConfig {
        id: id.to_string(),
        name: name.to_string(),
        ssh_host: "bastion.example.com".into(),
        ssh_port: 22,
        ssh_username: "deploy".into(),
        identity_file_path: None,
        has_stored_password: true,
        local_bind_address: "127.0.0.1".into(),
        local_port: 5432,
        remote_host: "db.internal".into(),
        remote_port: 5432,
        keep_alive_interval_sec: 30,
        keep_alive_max_count: 5,
        group_id: None,
        tags: vec!["prod".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(status: ForwardStatus) -> ForwardRuntime {
        ForwardRuntime {
            status,
            stats: TunnelStats::default(),
            last_error: None,
        }
    }

    #[test]
    fn request_round_trips_and_omits_absent_fields() {
        let req = CliRequest {
            v: PROTOCOL_VERSION,
            cmd: CliCommand::ConnectAll,
            target: None,
            timeout_ms: Some(45_000),
            wait: true,
        };
        let line = serde_json::to_string(&req).expect("serialize");
        assert!(
            line.contains("\"cmd\":\"connect-all\""),
            "kebab-case cmd: {line}"
        );
        assert!(
            line.contains("\"timeoutMs\":45000"),
            "camelCase field: {line}"
        );
        assert!(
            !line.contains("target"),
            "absent target must be omitted: {line}"
        );
        let back: CliRequest = serde_json::from_str(&line).expect("deserialize");
        assert_eq!(back, req);
    }

    #[test]
    fn request_defaults_wait_false_and_absent_optionals() {
        let req: CliRequest = serde_json::from_str(r#"{"v":1,"cmd":"list"}"#).expect("parse");
        assert_eq!(req.cmd, CliCommand::List);
        assert!(!req.wait);
        assert!(req.target.is_none());
        assert!(req.timeout_ms.is_none());
    }

    #[test]
    fn unknown_cmd_fails_to_deserialize() {
        let err = serde_json::from_str::<CliRequest>(r#"{"v":1,"cmd":"rm-rf"}"#);
        assert!(err.is_err(), "unknown cmd must not deserialize");
    }

    #[test]
    fn timeout_is_clamped_and_defaulted() {
        let mut req = CliRequest::new(CliCommand::Connect);
        assert_eq!(req.effective_timeout_ms(), DEFAULT_TIMEOUT_MS);
        req.timeout_ms = Some(1);
        assert_eq!(req.effective_timeout_ms(), MIN_TIMEOUT_MS);
        req.timeout_ms = Some(9_999_999);
        assert_eq!(req.effective_timeout_ms(), MAX_TIMEOUT_MS);
        req.timeout_ms = Some(45_000);
        assert_eq!(req.effective_timeout_ms(), 45_000);
    }

    #[test]
    fn forward_view_maps_config_and_runtime() {
        let cfg = sample_config("id-1", "prod-db");
        let view = ForwardView::from_parts(&cfg, &runtime(ForwardStatus::Connected));
        assert_eq!(view.id, "id-1");
        assert_eq!(view.name, "prod-db");
        assert_eq!(view.local_endpoint(), "127.0.0.1:5432");
        assert_eq!(view.remote_endpoint(), "db.internal:5432");
        assert_eq!(view.status, ForwardStatus::Connected);
        assert!(view.has_stored_password);
        assert_eq!(view.tags, vec!["prod".to_string()]);
    }

    /// AGENTS §8 / org policy: the socket must never be able to leak a secret.
    /// `ForwardConfig` has no password field today; this test fails loudly if a
    /// future refactor adds one and it reaches the wire through `ForwardView`.
    #[test]
    fn forward_view_json_carries_no_password_key() {
        let cfg = sample_config("id-1", "prod-db");
        let view = ForwardView::from_parts(&cfg, &runtime(ForwardStatus::Error));
        let value = serde_json::to_value(&view).expect("serialize view");
        let object = value.as_object().expect("view is a JSON object");
        for key in object.keys() {
            let lower = key.to_lowercase();
            assert!(
                !lower.contains("password") || key == "hasStoredPassword",
                "unexpected password-ish key on the wire: {key}"
            );
            assert!(!lower.contains("secret"), "unexpected secret key: {key}");
        }
        assert_eq!(
            object.get("hasStoredPassword").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn response_ok_and_err_round_trip() {
        let cfg = sample_config("id-1", "prod-db");
        let view = ForwardView::from_parts(&cfg, &runtime(ForwardStatus::Connected));
        let res = CliResponse::ok(CliData::Action {
            forward: view,
            waited: true,
            timed_out: false,
        });
        let line = serde_json::to_string(&res).expect("serialize");
        assert!(
            !line.contains("\"error\""),
            "ok response omits error: {line}"
        );
        let back: CliResponse = serde_json::from_str(&line).expect("deserialize");
        assert_eq!(back, res);

        let res = CliResponse::err(AppError::NotFound("forward x".into()));
        let line = serde_json::to_string(&res).expect("serialize");
        assert!(
            line.contains(r#""error":{"kind":"notFound","message":"forward x"}"#),
            "{line}"
        );
        assert!(
            !line.contains("\"data\""),
            "err response omits data: {line}"
        );
        let back: CliResponse = serde_json::from_str(&line).expect("deserialize");
        assert_eq!(back, res);
    }

    #[test]
    fn untagged_data_variants_round_trip_distinctly() {
        let cfg = sample_config("id-1", "prod-db");
        let view = ForwardView::from_parts(&cfg, &runtime(ForwardStatus::Connected));

        let variants = vec![
            CliData::List {
                forwards: vec![view.clone()],
            },
            CliData::Status {
                forward: view.clone(),
                stats: TunnelStats::default(),
            },
            CliData::Action {
                forward: view.clone(),
                waited: false,
                timed_out: false,
            },
            CliData::Bulk {
                results: vec![BulkResult {
                    id: "id-1".into(),
                    name: "prod-db".into(),
                    status: ForwardStatus::Connected,
                    last_error: None,
                    timed_out: false,
                }],
                succeeded: 1,
                failed: 0,
            },
            CliData::Version {
                app_version: "2.0.0".into(),
                protocol: PROTOCOL_VERSION,
            },
        ];
        for variant in variants {
            let line = serde_json::to_string(&variant).expect("serialize");
            let back: CliData = serde_json::from_str(&line).expect("deserialize");
            assert_eq!(back, variant, "variant lost its identity: {line}");
        }
    }

    #[test]
    fn status_labels_match_the_serde_wire_form() {
        for status in [
            ForwardStatus::Disconnected,
            ForwardStatus::Connecting,
            ForwardStatus::Connected,
            ForwardStatus::Disconnecting,
            ForwardStatus::Error,
        ] {
            let wire = serde_json::to_string(&status).expect("serialize status");
            assert_eq!(wire, format!("\"{}\"", status_label(status)));
        }
    }
}
