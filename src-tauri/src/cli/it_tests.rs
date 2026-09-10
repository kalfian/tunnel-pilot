//! In-process integration tests for the control socket (spec 03 §20
//! acceptance). A real `UnixListener` is bound in a `tempdir` and driven by a
//! real client connection, so the transport, the NDJSON framing, the socket
//! permissions and the service dispatch are all exercised together.
//!
//! `connect` is covered ONLY for an unresolvable target: a successful connect
//! would dial a real SSH host. The engine's own behaviour is covered by
//! `ssh/it_tests.rs` against an in-process russh server.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::cli::protocol::{
    sample_config, CliCommand, CliData, CliRequest, CliResponse, PROTOCOL_VERSION,
};
use crate::cli::server;
use crate::error::AppError;
use crate::state::models::ForwardStatus;
use crate::state::tunnel_registry::fake_handle;
use crate::state::AppState;

/// A bound, served socket plus the state behind it. The `tempdir` is held so
/// the socket path stays alive for the test's lifetime.
struct Harness {
    state: Arc<AppState>,
    path: PathBuf,
    _dir: tempfile::TempDir,
}

impl Harness {
    /// Two forwards; `id-a` ("prod-db") is live and connected.
    async fn start() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = Arc::new(AppState::new_headless());
        state.upsert_config(sample_config("id-a", "prod-db"));
        state.upsert_config(sample_config("id-b", "staging-redis"));
        state
            .registry
            .insert(fake_handle("id-a", ForwardStatus::Connected));

        let path = crate::cli::socket_path_in(dir.path());
        let listener = server::bind(&path).await.expect("bind control socket");
        tokio::spawn(server::serve_on(state.clone(), listener));

        Self {
            state,
            path,
            _dir: dir,
        }
    }

    /// Send one raw line, return the raw response line.
    async fn send_raw(&self, line: &str) -> String {
        send_raw_to(&self.path, line).await
    }

    /// Send a typed request, return the typed response.
    async fn send(&self, req: CliRequest) -> CliResponse {
        let line = serde_json::to_string(&req).expect("serialize request");
        let raw = self.send_raw(&line).await;
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("bad response {raw:?}: {e}"))
    }
}

async fn send_raw_to(path: &Path, line: &str) -> String {
    let stream = UnixStream::connect(path).await.expect("connect to socket");
    let (read_half, mut write_half) = stream.into_split();
    write_half
        .write_all(format!("{line}\n").as_bytes())
        .await
        .expect("write request");
    write_half.flush().await.expect("flush request");

    let mut reader = BufReader::new(read_half);
    let mut response = String::new();
    reader
        .read_line(&mut response)
        .await
        .expect("read response");
    response
}

fn target_request(cmd: CliCommand, target: &str) -> CliRequest {
    CliRequest {
        target: Some(target.to_string()),
        ..CliRequest::new(cmd)
    }
}

#[tokio::test]
async fn list_reports_every_forward_with_its_live_status() {
    let h = Harness::start().await;
    let res = h.send(CliRequest::new(CliCommand::List)).await;
    assert!(res.ok, "{res:?}");
    assert_eq!(res.v, PROTOCOL_VERSION);

    let Some(CliData::List { forwards }) = res.data else {
        panic!("expected a list payload, got {:?}", res.data);
    };
    assert_eq!(forwards.len(), 2);
    assert_eq!(forwards[0].id, "id-a");
    assert_eq!(forwards[0].status, ForwardStatus::Connected);
    assert_eq!(forwards[1].id, "id-b");
    assert_eq!(forwards[1].status, ForwardStatus::Disconnected);
}

#[tokio::test]
async fn status_resolves_a_name_case_insensitively() {
    let h = Harness::start().await;
    let res = h.send(target_request(CliCommand::Status, "PROD-db")).await;
    assert!(res.ok, "{res:?}");
    let Some(CliData::Status { forward, stats }) = res.data else {
        panic!("expected a status payload");
    };
    assert_eq!(forward.id, "id-a");
    assert_eq!(forward.status, ForwardStatus::Connected);
    assert_eq!(stats.active_connections, 0);
}

#[tokio::test]
async fn status_of_an_unknown_target_is_not_found() {
    let h = Harness::start().await;
    let raw = h
        .send_raw(r#"{"v":1,"cmd":"status","target":"ghost"}"#)
        .await;
    assert!(
        raw.contains(r#""ok":false"#) && raw.contains(r#""kind":"notFound""#),
        "unexpected response: {raw}"
    );
}

#[tokio::test]
async fn connect_of_an_unknown_target_is_not_found() {
    // The only `connect` we exercise end-to-end: an unresolvable target never
    // reaches the engine, so no SSH dial is attempted.
    let h = Harness::start().await;
    let res = h.send(target_request(CliCommand::Connect, "ghost")).await;
    assert!(!res.ok);
    assert!(matches!(res.error, Some(AppError::NotFound(_))), "{res:?}");
}

#[tokio::test]
async fn disconnect_tears_down_the_live_handle() {
    let h = Harness::start().await;
    assert!(h.state.registry.contains("id-a"));

    let res = h.send(target_request(CliCommand::Disconnect, "id-a")).await;
    assert!(res.ok, "{res:?}");
    let Some(CliData::Action {
        forward,
        waited,
        timed_out,
    }) = res.data
    else {
        panic!("expected an action payload");
    };
    assert_eq!(forward.status, ForwardStatus::Disconnected);
    assert!(waited && !timed_out);
    assert!(
        !h.state.registry.contains("id-a"),
        "the handle must be removed before the response"
    );
}

#[tokio::test]
async fn an_unsupported_protocol_version_is_rejected() {
    let h = Harness::start().await;
    let raw = h.send_raw(r#"{"v":99,"cmd":"list"}"#).await;
    assert!(
        raw.contains(r#""ok":false"#) && raw.contains(r#""kind":"invalidInput""#),
        "unexpected response: {raw}"
    );
    assert!(
        raw.contains("99"),
        "the error should name the version: {raw}"
    );
}

#[tokio::test]
async fn a_malformed_line_answers_with_invalid_input_and_keeps_serving() {
    let h = Harness::start().await;
    let raw = h.send_raw("not json at all").await;
    assert!(raw.contains(r#""kind":"invalidInput""#), "{raw}");

    // The listener survives a bad client.
    let res = h.send(CliRequest::new(CliCommand::List)).await;
    assert!(res.ok, "listener died after a malformed request: {res:?}");
}

#[tokio::test]
async fn one_connection_can_carry_several_requests() {
    let h = Harness::start().await;
    let stream = UnixStream::connect(&h.path).await.expect("connect");
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    for _ in 0..3 {
        write_half
            .write_all(b"{\"v\":1,\"cmd\":\"version\"}\n")
            .await
            .expect("write");
        let mut line = String::new();
        reader.read_line(&mut line).await.expect("read");
        assert!(line.contains(r#""ok":true"#), "{line}");
        assert!(line.contains(r#""protocol":1"#), "{line}");
    }
}

#[tokio::test]
async fn the_socket_is_owner_only_inside_an_owner_only_directory() {
    let h = Harness::start().await;
    let socket_mode = std::fs::metadata(&h.path)
        .expect("socket metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(socket_mode, 0o600, "socket must be owner read/write only");

    let dir_mode = std::fs::metadata(h.path.parent().expect("socket dir"))
        .expect("dir metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(dir_mode, 0o700, "socket dir must be owner-only");
}

#[tokio::test]
async fn a_stale_socket_file_is_replaced_on_bind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = crate::cli::socket_path_in(dir.path());
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    // A leftover file from a crashed instance: nothing is listening on it.
    std::fs::write(&path, b"stale").expect("write stale socket file");

    let listener = server::bind(&path).await.expect("bind over a stale socket");
    let state = Arc::new(AppState::new_headless());
    tokio::spawn(server::serve_on(state, listener));

    let raw = send_raw_to(&path, r#"{"v":1,"cmd":"list"}"#).await;
    assert!(raw.contains(r#""ok":true"#), "{raw}");
}

#[tokio::test]
async fn binding_over_a_live_socket_is_refused() {
    let h = Harness::start().await;
    // The single-instance plugin should prevent this, but if it ever happens we
    // must not steal the path from the instance that owns it.
    let err = server::bind(&h.path)
        .await
        .expect_err("a live socket must not be stolen");
    assert!(matches!(err, AppError::Internal(_)), "{err:?}");

    // The original listener still answers.
    let res = h.send(CliRequest::new(CliCommand::List)).await;
    assert!(res.ok, "{res:?}");
}

#[tokio::test]
async fn an_over_long_socket_path_fails_with_an_actionable_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("x".repeat(120)).join("cli.sock");
    let err = server::bind(&path).await.expect_err("must refuse to bind");
    assert!(err.to_string().contains(crate::cli::SOCKET_ENV), "{err}");
}

#[tokio::test]
async fn remove_socket_is_best_effort() {
    let h = Harness::start().await;
    server::remove_socket(&h.path);
    assert!(!h.path.exists(), "socket file should be gone");
    // A second removal (or a quit with no socket) must not panic.
    server::remove_socket(&h.path);
}

/// End-to-end over the real transport: the BLOCKING client (the code path a
/// terminal actually runs) against the real listener, asserting exit codes.
#[tokio::test]
async fn the_blocking_client_round_trips_against_the_listener() {
    use crate::cli::client;

    let h = Harness::start().await;
    let socket = h.path.display().to_string();

    let cli = |args: Vec<&str>| {
        let mut argv = vec!["--socket".to_string(), socket.clone()];
        argv.splice(0..0, args.iter().map(|a| a.to_string()));
        tokio::task::spawn_blocking(move || client::run(&argv))
    };

    assert_eq!(cli(vec!["list"]).await.expect("list"), client::EXIT_OK);
    assert_eq!(
        cli(vec!["status", "PROD-DB"]).await.expect("status"),
        client::EXIT_OK
    );
    assert_eq!(
        cli(vec!["status", "ghost"]).await.expect("unknown status"),
        client::EXIT_NOT_FOUND
    );
    assert_eq!(
        cli(vec!["list", "--json"]).await.expect("json list"),
        client::EXIT_OK
    );
    assert_eq!(
        cli(vec!["disconnect", "id-a"]).await.expect("disconnect"),
        client::EXIT_OK
    );
    assert!(!h.state.registry.contains("id-a"));
}
