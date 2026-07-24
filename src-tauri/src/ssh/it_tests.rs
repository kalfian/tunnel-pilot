//! Engine integration tests against an IN-PROCESS russh server (spec 03 §§1,2,
//! 5,6 acceptance). No external sshd/docker is required — a minimal russh 0.45
//! server that accepts password auth and forwards `direct-tcpip` channels to a
//! real TCP target runs inside the test process. If a future environment cannot
//! spawn the in-process server, these are the tests to gate behind `#[ignore]`.
//!
//! Coverage: end-to-end forward + byte counters (§1/§6), connection-lost via
//! session death → `error` with NO ping counter (F1/F7), **silent drop →
//! keepalive-timeout → `error`** (the real F1/F7 mechanism, via a black-hole
//! relay), dead-channel (3 forward failures) tears down + reconnects without
//! cancelling the parent (F26), teardown during `connecting` releases the local
//! port fast and reaches `disconnected` (F24/F31), **user disconnect during a
//! wedged RTT probe is still fast** (F32), retry from `error` reuses the same
//! supervisor (F23), **rapid connect/disconnect with no double-bind**,
//! **same-port conflict disconnects the first**, **disconnect while parked in
//! error**, and **retry hammering the live supervisor never loses a wakeup**
//! (F29).

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
    /// When true, stall session-channel opens (the client's RTT probe uses
    /// `channel_open_session`) — simulates a WEDGED session where the probe
    /// hangs to its timeout while TCP/keepalive stay alive (drives the F32 test).
    hang_session_channel: Arc<AtomicBool>,
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

    async fn channel_open_session(
        &mut self,
        _channel: russh::Channel<russh::server::Msg>,
        _session: &mut russh::server::Session,
    ) -> Result<bool, Self::Error> {
        // The client's 3s RTT probe opens a session channel. If wedged, delay
        // the confirmation past the client's probe timeout (F32).
        if self.hang_session_channel.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
        Ok(true)
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
    hang: Arc<AtomicBool>,
    accept_cancel: CancellationToken,
    handles: Arc<Mutex<Vec<russh::server::Handle>>>,
}

impl TestSsh {
    /// Kill the server: stop accepting AND disconnect every live session so
    /// connected clients see their session die (simulates `kill sshd` with a
    /// graceful SSH disconnect).
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

#[derive(Default)]
struct StartOpts {
    reject_channels: bool,
    hang_session_channel: bool,
    fixed_port: Option<u16>,
}

/// Start the SSH server. `fixed_port` lets the retry test rebind the same port.
async fn start_ssh_server(opts: StartOpts) -> TestSsh {
    let config = Arc::new(russh::server::Config {
        keys: vec![russh::keys::key::KeyPair::generate_ed25519().expect("keygen")],
        inactivity_timeout: Some(Duration::from_secs(3600)),
        ..Default::default()
    });
    let addr = format!("127.0.0.1:{}", opts.fixed_port.unwrap_or(0));
    let listener = TcpListener::bind(&addr).await.expect("bind ssh");
    let port = listener.local_addr().unwrap().port();
    let reject = Arc::new(AtomicBool::new(opts.reject_channels));
    let hang = Arc::new(AtomicBool::new(opts.hang_session_channel));
    let accept_cancel = CancellationToken::new();
    let handles: Arc<Mutex<Vec<russh::server::Handle>>> = Arc::new(Mutex::new(Vec::new()));

    let cfg = config.clone();
    let reject_c = reject.clone();
    let hang_c = hang.clone();
    let cancel_c = accept_cancel.clone();
    let handles_c = handles.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel_c.cancelled() => break,
                accept = listener.accept() => {
                    let Ok((socket, _)) = accept else { break };
                    let cfg = cfg.clone();
                    let handler = TestServerHandler {
                        reject_channels: reject_c.clone(),
                        hang_session_channel: hang_c.clone(),
                    };
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
        hang,
        accept_cancel,
        handles,
    }
}

/// A switchable TCP relay: forwards `client <-> 127.0.0.1:target` until put into
/// "black-hole" mode, after which it stops forwarding but HOLDS both sockets
/// open (no FIN/RST) — simulating a silent network death (VPN drop). With the
/// SSH connection black-holed, russh keepalives get no reply and the client
/// session ends via keepalive-timeout (the real F1/F7 liveness mechanism).
struct Relay {
    port: u16,
    blackhole: Arc<AtomicBool>,
    _cancel: CancellationToken,
}

async fn start_relay(target: u16) -> Relay {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind relay");
    let port = listener.local_addr().unwrap().port();
    let blackhole = Arc::new(AtomicBool::new(false));
    let cancel = CancellationToken::new();
    let bh = blackhole.clone();
    let cancel_c = cancel.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel_c.cancelled() => break,
                accept = listener.accept() => {
                    let Ok((client, _)) = accept else { break };
                    let Ok(server) = TcpStream::connect(("127.0.0.1", target)).await else { continue };
                    let (cr, cw) = client.into_split();
                    let (sr, sw) = server.into_split();
                    let bh1 = bh.clone();
                    let bh2 = bh.clone();
                    let cc1 = cancel_c.clone();
                    let cc2 = cancel_c.clone();
                    tokio::spawn(relay_dir(cr, sw, bh1, cc1));
                    tokio::spawn(relay_dir(sr, cw, bh2, cc2));
                }
            }
        }
    });
    Relay {
        port,
        blackhole,
        _cancel: cancel,
    }
}

async fn relay_dir(
    mut r: tokio::net::tcp::OwnedReadHalf,
    mut w: tokio::net::tcp::OwnedWriteHalf,
    blackhole: Arc<AtomicBool>,
    cancel: CancellationToken,
) {
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        if blackhole.load(Ordering::SeqCst) {
            // Hold the socket halves open, forward nothing (silent death).
            cancel.cancelled().await;
            return;
        }
        tokio::select! {
            _ = cancel.cancelled() => return,
            _ = tokio::time::sleep(Duration::from_millis(40)) => {}
            res = r.read(&mut buf) => {
                match res {
                    Ok(0) | Err(_) => return,
                    Ok(n) => {
                        if w.write_all(&buf[..n]).await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
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
    let ssh = start_ssh_server(StartOpts::default()).await;
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
    let ssh = start_ssh_server(StartOpts::default()).await;
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
    let ssh = start_ssh_server(StartOpts {
        reject_channels: true,
        ..StartOpts::default()
    })
    .await;
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
    let ssh = start_ssh_server(StartOpts::default()).await;
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
    let ssh2 = start_ssh_server(StartOpts {
        fixed_port: Some(ssh_port),
        ..StartOpts::default()
    })
    .await;

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

#[tokio::test]
async fn user_disconnect_during_wedged_probe_is_fast() {
    use crate::state::models::ForwardStatus;

    // F32: the RTT probe hangs on a wedged session. A user disconnect must NOT
    // wait for the ~3s probe timeout — with the spawned-probe fix the accept
    // loop stays responsive to attempt-cancel.
    let echo = start_echo_target().await;
    let ssh = start_ssh_server(StartOpts {
        hang_session_channel: true,
        ..StartOpts::default()
    })
    .await;
    let local_port = free_port().await;

    // Keepalive slow so the session itself doesn't die during the test window —
    // we want ONLY the RTT probe to hang, not the session.
    let state = make_state(false, 0);
    let mut cfg = base_config("wedge", local_port, ssh.port, echo.port);
    cfg.keep_alive_interval_sec = 3600;
    cfg.keep_alive_max_count = 100;
    state.upsert_config(cfg);
    state.set_password("wedge", "pw".into());

    engine::connect_forward(&state, "wedge").await.unwrap();
    assert!(
        wait_status(
            &state,
            "wedge",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );

    // Let a probe get in flight (it will hang to 3s in its own spawned task).
    tokio::time::sleep(Duration::from_millis(200)).await;

    let t0 = Instant::now();
    engine::disconnect_forward(&state, "wedge", true)
        .await
        .unwrap();
    let elapsed = t0.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "user disconnect during a wedged probe must be fast, took {elapsed:?}"
    );
    assert!(state.registry.current_status("wedge").is_none());

    ssh.kill().await;
}

#[tokio::test]
async fn silent_drop_triggers_keepalive_timeout() {
    use crate::state::models::ForwardStatus;

    // F35 (the REAL F1/F7 mechanism): black-hole the SSH TCP connection WITHOUT
    // a graceful disconnect. russh keepalives get no reply → after
    // keepalive_max the client session ends via keepalive-timeout → error.
    let echo = start_echo_target().await;
    let ssh = start_ssh_server(StartOpts::default()).await;
    let relay = start_relay(ssh.port).await;
    let local_port = free_port().await;

    // auto_reconnect OFF so the tunnel parks in error once the session dies.
    let state = make_state(false, 0);
    // ssh_host/port point at the RELAY; keepalive 1s × 2 → dead within ~2-4s.
    let mut cfg = base_config("silent", local_port, relay.port, echo.port);
    cfg.keep_alive_interval_sec = 1;
    cfg.keep_alive_max_count = 2;
    state.upsert_config(cfg);
    state.set_password("silent", "pw".into());

    engine::connect_forward(&state, "silent").await.unwrap();
    assert!(
        wait_status(
            &state,
            "silent",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );

    // Silent death: stop forwarding but hold sockets open (no FIN/RST).
    relay.blackhole.store(true, Ordering::SeqCst);

    // Must reach error within ~ keepalive_interval × keepalive_max (+ margin),
    // proving liveness comes from keepalive-timeout, not a graceful disconnect
    // and NOT any app-level ping counter (there is none).
    assert!(
        wait_status(
            &state,
            "silent",
            ForwardStatus::Error,
            Duration::from_secs(12)
        )
        .await,
        "silent drop should trip keepalive-timeout → error"
    );

    engine::disconnect_forward(&state, "silent", true)
        .await
        .unwrap();
    ssh.kill().await;
}

#[tokio::test]
async fn rapid_connect_disconnect_connect_no_double_bind() {
    use crate::state::models::ForwardStatus;

    let echo = start_echo_target().await;
    let ssh = start_ssh_server(StartOpts::default()).await;
    let local_port = free_port().await;

    let state = make_state(false, 0);
    state.upsert_config(base_config("rapid", local_port, ssh.port, echo.port));
    state.set_password("rapid", "pw".into());

    // Several fast connect→disconnect cycles on the SAME local port must never
    // double-bind (F25/F21): each connect binds only after the prior release.
    for _ in 0..4 {
        engine::connect_forward(&state, "rapid").await.unwrap();
        assert!(
            wait_status(
                &state,
                "rapid",
                ForwardStatus::Connected,
                Duration::from_secs(10)
            )
            .await,
            "each cycle should connect (no EADDRINUSE)"
        );
        engine::disconnect_forward(&state, "rapid", true)
            .await
            .unwrap();
        assert!(state.registry.current_status("rapid").is_none());
    }

    // No leaked listener on the port after the final disconnect.
    let rebind = TcpListener::bind(("127.0.0.1", local_port)).await;
    assert!(rebind.is_ok(), "port free after rapid cycles");

    ssh.kill().await;
}

#[tokio::test]
async fn same_port_conflict_disconnects_first() {
    use crate::state::models::ForwardStatus;

    let echo = start_echo_target().await;
    let ssh = start_ssh_server(StartOpts::default()).await;
    let local_port = free_port().await;

    let state = make_state(false, 0);
    // Two DISTINCT configs sharing the same local port.
    state.upsert_config(base_config("first", local_port, ssh.port, echo.port));
    state.set_password("first", "pw".into());
    state.upsert_config(base_config("second", local_port, ssh.port, echo.port));
    state.set_password("second", "pw".into());

    engine::connect_forward(&state, "first").await.unwrap();
    assert!(
        wait_status(
            &state,
            "first",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );

    // Connecting the second (same port) must disconnect the first (spec 03 §1).
    engine::connect_forward(&state, "second").await.unwrap();
    assert!(
        wait_status(
            &state,
            "second",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );
    assert!(
        state.registry.current_status("first").is_none(),
        "the first tunnel on the shared port was disconnected"
    );

    engine::disconnect_forward(&state, "second", true)
        .await
        .unwrap();
    ssh.kill().await;
}

#[tokio::test]
async fn disconnect_while_parked_in_error() {
    use crate::state::models::ForwardStatus;

    // End-to-end: a tunnel parked in `error` (auto_reconnect off) can be
    // disconnected by the user and reaches disconnected/removed (F31/F23).
    let echo = start_echo_target().await;
    let ssh = start_ssh_server(StartOpts::default()).await;
    let local_port = free_port().await;

    let state = make_state(false, 0);
    state.upsert_config(base_config("parked", local_port, ssh.port, echo.port));
    state.set_password("parked", "pw".into());

    engine::connect_forward(&state, "parked").await.unwrap();
    assert!(
        wait_status(
            &state,
            "parked",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );
    ssh.kill().await;
    assert!(
        wait_status(
            &state,
            "parked",
            ForwardStatus::Error,
            Duration::from_secs(10)
        )
        .await,
        "parks in error after the drop"
    );

    // User disconnect while parked → cancels parent, awaits supervisor, removes.
    engine::disconnect_forward(&state, "parked", true)
        .await
        .unwrap();
    assert!(
        state.registry.current_status("parked").is_none(),
        "disconnect from error reaches disconnected/removed"
    );
    // Port released.
    assert!(TcpListener::bind(("127.0.0.1", local_port)).await.is_ok());
}

#[tokio::test]
async fn retry_racing_reconnect_attempts_never_lost() {
    use crate::state::models::ForwardStatus;

    // End-to-end F29/F23: hammer retry_forward through the LIVE supervisor while
    // the server is down (each retry races the re-entry into error-park), then
    // bring the server back and fire a final retry — the tunnel must reconnect,
    // proving no retry-wakeup is permanently lost and the supervisor is reused.
    let echo = start_echo_target().await;
    let ssh = start_ssh_server(StartOpts::default()).await;
    let ssh_port = ssh.port;
    let local_port = free_port().await;

    // auto_reconnect OFF → each failed attempt parks in error, so retries drive
    // the attempts (exercises the flag/notify racing path).
    let state = make_state(false, 0);
    state.upsert_config(base_config("race", local_port, ssh_port, echo.port));
    state.set_password("race", "pw".into());

    engine::connect_forward(&state, "race").await.unwrap();
    assert!(
        wait_status(
            &state,
            "race",
            ForwardStatus::Connected,
            Duration::from_secs(10)
        )
        .await
    );

    ssh.kill().await;
    assert!(
        wait_status(
            &state,
            "race",
            ForwardStatus::Error,
            Duration::from_secs(10)
        )
        .await
    );

    // Hammer retries while the server is DOWN: each wakes the supervisor, which
    // attempts connect (fails fast: connection refused) and re-parks in error.
    // None of these should panic, leak, or wedge the supervisor.
    for _ in 0..20 {
        engine::retry_forward(&state, "race").await.unwrap();
        tokio::time::sleep(Duration::from_millis(15)).await;
    }
    assert!(
        state.registry.contains("race"),
        "supervisor still alive (reused)"
    );

    // Bring the server back on the same port and fire a final retry.
    let ssh2 = start_ssh_server(StartOpts {
        fixed_port: Some(ssh_port),
        ..StartOpts::default()
    })
    .await;
    // Ensure we are parked in error before the deciding retry.
    let _ = wait_status(&state, "race", ForwardStatus::Error, Duration::from_secs(5)).await;
    engine::retry_forward(&state, "race").await.unwrap();
    assert!(
        wait_status(
            &state,
            "race",
            ForwardStatus::Connected,
            Duration::from_secs(15)
        )
        .await,
        "a retry after the server returns must reconnect (no lost wakeup)"
    );

    engine::disconnect_forward(&state, "race", true)
        .await
        .unwrap();
    ssh2.kill().await;
}
