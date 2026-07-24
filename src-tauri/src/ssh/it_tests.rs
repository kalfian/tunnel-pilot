//! Engine integration tests against an IN-PROCESS russh server (spec 03 §§1,2,
//! 5,6 acceptance). No external sshd/docker is required — a minimal russh 0.45
//! server that accepts password auth and forwards `direct-tcpip` channels to a
//! real TCP target runs inside the test process. If a future environment cannot
//! spawn the in-process server, these are the tests to gate behind `#[ignore]`.
//!
//! Coverage: end-to-end forward + byte counters (§1/§6), connection-lost via
//! session death → `error` with NO ping counter (F1/F7), dead-channel (3
//! forward failures) tears down + reconnects without cancelling the parent
//! (F26), teardown during `connecting` releases the local port fast and reaches
//! `disconnected` (F24/F31), and retry from `error` reuses the same supervisor
//! (F23).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;

use crate::ssh::engine;
use crate::state::models::{AppSettings, ForwardConfig};
use crate::state::AppState;

// ----------------------------------------------------------------------------
// Test harness: a plain-TCP echo target + a minimal russh forwarding server.
// ----------------------------------------------------------------------------

/// A TCP echo server used as the forward's `remote` target.
struct EchoTarget {
    port: u16,
    _cancel: CancellationToken,
}

async fn start_echo_target() -> EchoTarget {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind echo");
    let port = listener.local_addr().unwrap().port();
    let cancel = CancellationToken::new();
    let c = cancel.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = c.cancelled() => break,
                accept = listener.accept() => {
                    let Ok((mut sock, _)) = accept else { break };
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        loop {
                            match sock.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => {
                                    if sock.write_all(&buf[..n]).await.is_err() {
                                        break;
                                    }
                                }
                            }
                        }
                    });
                }
            }
        }
    });
    EchoTarget {
        port,
        _cancel: cancel,
    }
}

/// Server-side handler: accept password auth and forward direct-tcpip channels.
#[derive(Clone)]
struct TestServerHandler {
    /// When true, refuse to open forwarded channels (drives the dead-channel
    /// test: the client's `channel_open_direct_tcpip` errors).
    reject_channels: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl russh::server::Handler for TestServerHandler {
    type Error = russh::Error;

    async fn auth_password(
        &mut self,
        _user: &str,
        _password: &str,
    ) -> Result<russh::server::Auth, Self::Error> {
        Ok(russh::server::Auth::Accept)
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: russh::Channel<russh::server::Msg>,
        host_to_connect: &str,
        port_to_connect: u32,
        _originator_address: &str,
        _originator_port: u32,
        _session: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        if self.reject_channels.load(Ordering::SeqCst) {
            return Ok(false); // reject → client open errors → forward failure
        }
        let target = format!("{host_to_connect}:{port_to_connect}");
        tokio::spawn(async move {
            if let Ok(mut tcp) = TcpStream::connect(&target).await {
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut tcp).await;
            }
        });
        Ok(true)
    }
}

/// A running in-process SSH server with a kill switch.
struct TestSsh {
    port: u16,
    reject: Arc<AtomicBool>,
    accept_cancel: CancellationToken,
    handles: Arc<Mutex<Vec<russh::server::Handle>>>,
}

impl TestSsh {
    /// Kill the server: stop accepting AND disconnect every live session so
    /// connected clients see their session die (simulates `kill sshd`).
    async fn kill(&self) {
        self.accept_cancel.cancel();
        let handles = {
            let mut g = self.handles.lock().unwrap();
            std::mem::take(&mut *g)
        };
        for h in handles {
            let _ = h
                .disconnect(russh::Disconnect::ByApplication, "".into(), "".into())
                .await;
        }
    }
}

/// Start the SSH server on an ephemeral port (or `fixed_port` if given, for the
/// retry test that must rebind the same port).
async fn start_ssh_server(reject_channels: bool, fixed_port: Option<u16>) -> TestSsh {
    let config = Arc::new(russh::server::Config {
        keys: vec![russh::keys::key::KeyPair::generate_ed25519().expect("keygen")],
        inactivity_timeout: Some(Duration::from_secs(3600)),
        ..Default::default()
    });
    let addr = format!("127.0.0.1:{}", fixed_port.unwrap_or(0));
    let listener = TcpListener::bind(&addr).await.expect("bind ssh");
    let port = listener.local_addr().unwrap().port();
    let reject = Arc::new(AtomicBool::new(reject_channels));
    let accept_cancel = CancellationToken::new();
    let handles: Arc<Mutex<Vec<russh::server::Handle>>> = Arc::new(Mutex::new(Vec::new()));

    let cfg = config.clone();
    let reject_c = reject.clone();
    let cancel_c = accept_cancel.clone();
    let handles_c = handles.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel_c.cancelled() => break,
                accept = listener.accept() => {
                    let Ok((socket, _)) = accept else { break };
                    let cfg = cfg.clone();
                    let handler = TestServerHandler { reject_channels: reject_c.clone() };
                    let handles_inner = handles_c.clone();
                    tokio::spawn(async move {
                        if let Ok(running) = russh::server::run_stream(cfg, socket, handler).await {
                            handles_inner.lock().unwrap().push(running.handle());
                            let _ = running.await; // drive the session to completion
                        }
                    });
                }
            }
        }
    });

    TestSsh {
        port,
        reject,
        accept_cancel,
        handles,
    }
}

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn base_config(id: &str, local_port: u16, ssh_port: u16, remote_port: u16) -> ForwardConfig {
    ForwardConfig {
        id: id.to_string(),
        name: id.to_string(),
        ssh_host: "127.0.0.1".to_string(),
        ssh_port,
        ssh_username: "tester".to_string(),
        identity_file_path: None,
        has_stored_password: true,
        local_bind_address: "127.0.0.1".to_string(),
        local_port,
        remote_host: "127.0.0.1".to_string(),
        remote_port,
        // Fast keepalive so a dead peer is detected quickly in tests.
        keep_alive_interval_sec: 1,
        keep_alive_max_count: 2,
        group_id: None,
        tags: vec![],
    }
}

/// Pick a free local TCP port by binding an ephemeral socket and releasing it.
async fn free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().port()
}

async fn wait_status(
    state: &Arc<AppState>,
    id: &str,
    target: crate::state::models::ForwardStatus,
    within: Duration,
) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if state.registry.current_status(id) == Some(target) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn make_state(auto_reconnect: bool, max_retries: u32) -> Arc<AppState> {
    let state = Arc::new(AppState::new_headless());
    {
        let mut s = state.settings.write().unwrap();
        *s = AppSettings {
            auto_reconnect,
            auto_reconnect_delay_sec: 1,
            auto_reconnect_max_retries: max_retries,
            ..AppSettings::default()
        };
    }
    state
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[tokio::test]
async fn end_to_end_forward_and_byte_counters() {
    use crate::state::models::ForwardStatus;

    let echo = start_echo_target().await;
    let ssh = start_ssh_server(false, None).await;
    let local_port = free_port().await;

    let state = make_state(false, 0);
    let cfg = base_config("e2e", local_port, ssh.port, echo.port);
    state.upsert_config(cfg);
    state.set_password("e2e", "pw".into());

    engine::connect_forward(&state, "e2e").await.unwrap();
    assert!(
        wait_status(
            &state,
            "e2e",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await,
        "tunnel should reach connected"
    );

    // Drive traffic through the tunnel and verify the echo round-trips.
    let mut client = TcpStream::connect(("127.0.0.1", local_port))
        .await
        .expect("connect local");
    let payload = b"hello-tunnel-pilot";
    client.write_all(payload).await.unwrap();
    let mut buf = vec![0u8; payload.len()];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, payload, "echo round-trip");

    // Byte counters reflect the transfer (both directions >= payload).
    // Give the down-copy a moment to account the echoed bytes.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let rt = state.registry.runtime("e2e").unwrap();
    assert!(rt.stats.total_bytes_up >= payload.len() as u64, "bytes up");
    assert!(
        rt.stats.total_bytes_down >= payload.len() as u64,
        "bytes down"
    );
    assert!(rt.stats.active_connections >= 1, "one live connection");

    drop(client);

    // Disconnect releases the local port before the entry is gone (F21).
    engine::disconnect_forward(&state, "e2e", true)
        .await
        .unwrap();
    assert!(
        state.registry.current_status("e2e").is_none(),
        "entry removed"
    );
    // Immediate rebind on the same port must not hit EADDRINUSE.
    let rebind = TcpListener::bind(("127.0.0.1", local_port)).await;
    assert!(rebind.is_ok(), "local port released after disconnect");

    ssh.kill().await;
}

#[tokio::test]
async fn session_death_moves_to_error_no_ping_counter() {
    use crate::state::models::ForwardStatus;

    // auto_reconnect OFF → a dropped session parks in `error` (not reconnect).
    let echo = start_echo_target().await;
    let ssh = start_ssh_server(false, None).await;
    let local_port = free_port().await;

    let state = make_state(false, 0);
    state.upsert_config(base_config("kill", local_port, ssh.port, echo.port));
    state.set_password("kill", "pw".into());

    engine::connect_forward(&state, "kill").await.unwrap();
    assert!(
        wait_status(
            &state,
            "kill",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );

    // Kill the server → the russh session future ends (F7) → status error.
    // There is NO app-level ping counter anywhere in the engine (F1) — the
    // signal is is_closed()/keepalive, verified by reaching `error` here.
    ssh.kill().await;
    assert!(
        wait_status(
            &state,
            "kill",
            ForwardStatus::Error,
            Duration::from_secs(10)
        )
        .await,
        "session death should move tunnel to error"
    );
    let rt = state.registry.runtime("kill").unwrap();
    assert!(rt.last_error.is_some(), "error carries a message");

    engine::disconnect_forward(&state, "kill", true)
        .await
        .unwrap();
}

#[tokio::test]
async fn dead_channel_reconnects_without_cancelling_parent() {
    use crate::state::models::ForwardStatus;

    let echo = start_echo_target().await;
    // Start REJECTING forwarded channels so opens fail.
    let ssh = start_ssh_server(true, None).await;
    let local_port = free_port().await;

    // auto_reconnect ON so the dead-channel teardown reconnects (F26).
    let state = make_state(true, 10);
    state.upsert_config(base_config("dead", local_port, ssh.port, echo.port));
    state.set_password("dead", "pw".into());

    engine::connect_forward(&state, "dead").await.unwrap();
    assert!(
        wait_status(
            &state,
            "dead",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );
    let parent = state.registry.parent_token("dead").unwrap();

    // Trigger 3 failed forward opens (rejected channels) → dead-channel teardown.
    for _ in 0..3 {
        if let Ok(c) = TcpStream::connect(("127.0.0.1", local_port)).await {
            let _ = c;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // The tunnel must leave `connected` (teardown), and the parent must NOT be
    // cancelled (F26: dead-channel is a reconnect, not a permanent kill).
    let left_connected = {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let s = state.registry.current_status("dead");
            if s != Some(ForwardStatus::Connected) {
                break true;
            }
            if Instant::now() >= deadline {
                break false;
            }
            // keep poking connections to accrue failures
            if let Ok(c) = TcpStream::connect(("127.0.0.1", local_port)).await {
                let _ = c;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    };
    assert!(
        left_connected,
        "3 forward failures should tear the tunnel down"
    );
    assert!(
        !parent.is_cancelled(),
        "dead-channel must NOT cancel the parent (F26)"
    );
    assert!(state.registry.contains("dead"), "supervisor still alive");

    // Now allow channels; the auto-reconnect should recover to connected.
    ssh.reject.store(false, Ordering::SeqCst);
    assert!(
        wait_status(
            &state,
            "dead",
            ForwardStatus::Connected,
            Duration::from_secs(15)
        )
        .await,
        "tunnel should reconnect after the fault clears"
    );

    engine::disconnect_forward(&state, "dead", true)
        .await
        .unwrap();
    ssh.kill().await;
}

#[tokio::test]
async fn teardown_during_connecting_releases_port_fast() {
    use crate::state::models::ForwardStatus;

    // A plain TCP listener that accepts but NEVER speaks SSH → the client hangs
    // in the handshake (up to the 15s connect timeout). A disconnect during
    // `connecting` must bail within cancel latency, not 15s (F24), and reach
    // `disconnected` (F31).
    let dead = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead_port = dead.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((sock, _)) = dead.accept().await {
            // Hold the socket open, say nothing (never complete the SSH handshake).
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(60)).await;
                drop(sock);
            });
        }
    });

    let local_port = free_port().await;
    let state = make_state(false, 0);
    state.upsert_config(base_config("hang", local_port, dead_port, 9));
    state.set_password("hang", "pw".into());

    engine::connect_forward(&state, "hang").await.unwrap();
    assert!(
        wait_status(
            &state,
            "hang",
            ForwardStatus::Connecting,
            Duration::from_secs(5)
        )
        .await,
        "should be connecting (blocked in handshake)"
    );

    // Disconnect mid-connect; it must return quickly (F24) and reach disconnected.
    let t0 = Instant::now();
    engine::disconnect_forward(&state, "hang", true)
        .await
        .unwrap();
    let elapsed = t0.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "teardown during connecting must be bounded by cancel latency, took {elapsed:?}"
    );
    assert!(
        state.registry.current_status("hang").is_none(),
        "reached disconnected/removed"
    );
    // Local port released promptly (F24) — nothing was ever bound long, but the
    // listener that WAS bound during connecting must be gone.
    let rebind = TcpListener::bind(("127.0.0.1", local_port)).await;
    assert!(rebind.is_ok(), "local port free after fast teardown");
}

#[tokio::test]
async fn retry_from_error_reuses_same_supervisor() {
    use crate::state::models::ForwardStatus;

    let echo = start_echo_target().await;
    let ssh = start_ssh_server(false, None).await;
    let ssh_port = ssh.port;
    let local_port = free_port().await;

    // auto_reconnect OFF → after the drop it parks in error, awaiting a retry.
    let state = make_state(false, 0);
    state.upsert_config(base_config("retry", local_port, ssh_port, echo.port));
    state.set_password("retry", "pw".into());

    engine::connect_forward(&state, "retry").await.unwrap();
    assert!(
        wait_status(
            &state,
            "retry",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );

    // Kill the server → parks in error.
    ssh.kill().await;
    assert!(
        wait_status(
            &state,
            "retry",
            ForwardStatus::Error,
            Duration::from_secs(10)
        )
        .await
    );
    // Same supervisor is still registered (parked, not removed) — F23.
    assert!(
        state.registry.contains("retry"),
        "supervisor parked, entry intact"
    );

    // Bring a fresh server up on the SAME port so a retry can succeed.
    let ssh2 = start_ssh_server(false, Some(ssh_port)).await;

    // Retry from error reuses the same supervisor (no respawn) → reconnects.
    engine::retry_forward(&state, "retry").await.unwrap();
    assert!(
        wait_status(
            &state,
            "retry",
            ForwardStatus::Connected,
            Duration::from_secs(15)
        )
        .await,
        "retry from error should reconnect"
    );
    assert!(state.registry.contains("retry"));

    engine::disconnect_forward(&state, "retry", true)
        .await
        .unwrap();
    ssh2.kill().await;
}
