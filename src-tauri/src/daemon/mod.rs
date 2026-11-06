//! `tunnel-pilot-cli daemon` — the headless owner of `AppState` (spec 03 §21).
//!
//! The desktop app is one owner of the tunnel engine; this is the other, for
//! hosts with no display (servers, containers, a Pi). It boots exactly what the
//! desktop `setup` hook boots minus every window concern — config + migration
//! (`boot.rs`), the credential store, `AppState`, the sleep/wake watchdog and
//! the CLI control socket — and then serves the socket until SIGINT/SIGTERM.
//! Every CLI command works against it unchanged: the socket protocol does not
//! know or care which owner answers.
//!
//! Single owner: the socket is bound BEFORE any tunnel starts, and binding
//! refuses a socket something is still answering on (`server::bind`). So a
//! second daemon — or a daemon while the desktop app is running — exits 1
//! instead of fighting over the same ports and config file.

pub mod service;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::cli::args::DaemonOptions;
use crate::cli::client::{EXIT_INTERNAL, EXIT_OK, EXIT_USAGE};
use crate::credentials::CredentialStore;
use crate::ops::forwards::{run_start_all, run_stop_all};
use crate::state::log_buffer::LogBuffer;
use crate::state::AppState;
use crate::storage::config_file::ConfigStore;

/// Upper bound on the graceful disconnect at shutdown; a wedged SSH peer must
/// not keep systemd/launchd waiting for its own kill timeout.
const SHUTDOWN_BUDGET: Duration = Duration::from_secs(10);

/// Run the daemon in the foreground; returns the process exit code.
pub fn run(options: &DaemonOptions) -> i32 {
    let Some(config_dir) = crate::cli::default_config_dir() else {
        eprintln!(
            "error: could not resolve the config dir; set {}",
            crate::cli::CONFIG_DIR_ENV
        );
        return EXIT_INTERNAL;
    };
    match crate::rt::block_on(serve(options.clone(), config_dir)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: could not start the async runtime: {e}");
            EXIT_INTERNAL
        }
    }
}

async fn serve(options: DaemonOptions, config_dir: PathBuf) -> i32 {
    // Same log plumbing as the desktop app: the ring buffer feeds `get_logs`
    // parity, the fmt layer writes to stdout (→ journald / launchd log file).
    let logs = Arc::new(LogBuffer::new());
    crate::logging::set_log_buffer(logs.clone());
    crate::logging::init_tracing_for_daemon();
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        edition = crate::cli::EDITION,
        config_dir = %config_dir.display(),
        "Tunnel Pilot daemon starting"
    );

    if let Err(e) = tokio::fs::create_dir_all(&config_dir).await {
        tracing::error!(error = %e, "cannot create the config dir");
        return EXIT_INTERNAL;
    }

    // Validate the web options BEFORE claiming anything: a bad flag or a
    // missing admin password must not leave a half-started daemon.
    let web_addr = if options.web {
        if !crate::web::admin::password_is_set(&config_dir) {
            eprintln!(
                "error: --web needs an admin password first: tunnel-pilot-cli web set-password"
            );
            return EXIT_USAGE;
        }
        match crate::web::parse_bind(options.bind.as_deref()) {
            Ok(addr) => Some(addr),
            Err(e) => {
                eprintln!("error: {e}");
                return EXIT_USAGE;
            }
        }
    } else {
        None
    };

    // Claim the socket first: it is the single-owner lock (see module docs).
    let socket_path = crate::cli::resolved_socket_path(&config_dir);
    let listener = match crate::cli::server::bind(&socket_path).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!(socket = %socket_path.display(), error = %e, "cannot own the control socket");
            eprintln!("error: {e}");
            if e.to_string().contains("already serving") {
                eprintln!(
                    "The desktop app or another daemon already owns the tunnels. \
                     Check with: tunnel-pilot-cli list"
                );
            }
            return EXIT_INTERNAL;
        }
    };

    let state = match build_state(&config_dir, logs).await {
        Ok(state) => state,
        Err(e) => {
            tracing::error!(error = %e, "daemon boot failed");
            crate::cli::server::remove_socket(&socket_path);
            return EXIT_INTERNAL;
        }
    };

    crate::ssh::wake::spawn_wake_watchdog(state.clone());
    let server = tokio::spawn(crate::cli::server::serve_on(state.clone(), listener));

    let web = match web_addr {
        Some(addr) => match crate::web::start(
            state.clone(),
            config_dir.clone(),
            addr,
            crate::web::WebOptions {
                trust_proxy: options.trust_proxy,
                public_hosts: options.public_hosts.clone(),
            },
        )
        .await
        {
            Ok((bound, task)) => {
                announce_web(bound);
                Some(task)
            }
            Err(e) => {
                tracing::error!(error = %e, "web UI failed to start");
                eprintln!("error: {e}");
                server.abort();
                crate::cli::server::remove_socket(&socket_path);
                return EXIT_INTERNAL;
            }
        },
        None => None,
    };
    tracing::info!(
        socket = %socket_path.display(),
        forwards = state.configs_snapshot().len(),
        "daemon ready"
    );

    connect_on_start(&state, &options).await;

    wait_for_shutdown_signal().await;
    tracing::info!("shutdown requested; disconnecting tunnels");
    server.abort();
    if let Some(web) = web {
        web.abort();
    }
    if tokio::time::timeout(SHUTDOWN_BUDGET, run_stop_all(&state))
        .await
        .is_err()
    {
        tracing::warn!("tunnels did not all disconnect within the shutdown budget");
    }
    crate::cli::server::remove_socket(&socket_path);
    tracing::info!("daemon stopped");
    EXIT_OK
}

/// Log where the web UI listens, and warn when it is reachable beyond this
/// machine over plain HTTP (the admin password would cross the network in
/// clear text unless a TLS reverse proxy fronts it).
fn announce_web(addr: SocketAddr) {
    tracing::info!(url = %format!("http://{addr}/"), "web UI listening");
    if !addr.ip().is_loopback() {
        tracing::warn!(
            bind = %addr,
            "web UI is reachable from the network over plain HTTP — put an HTTPS reverse \
             proxy (Caddy, nginx) in front of it, or bind 127.0.0.1 and use an SSH tunnel"
        );
    }
}

/// Load config + credentials and build the shared state (the desktop `setup`
/// hook's sequence, minus the window).
async fn build_state(
    config_dir: &Path,
    logs: Arc<LogBuffer>,
) -> Result<Arc<AppState>, crate::error::AppError> {
    let config_store = Arc::new(ConfigStore::from_config_dir(config_dir));
    // The keychain probe is a blocking round-trip; keep it off the runtime.
    let dir = config_dir.to_path_buf();
    let credentials = Arc::new(
        tokio::task::spawn_blocking(move || CredentialStore::from_app_dir(&dir))
            .await
            .map_err(|e| crate::error::AppError::Internal(format!("credential probe: {e}")))?,
    );
    let doc = crate::boot::load_document(config_store.as_ref(), credentials.as_ref()).await;
    Ok(Arc::new(AppState::new_persistent(
        config_store,
        credentials,
        logs,
        doc,
    )))
}

/// `--connect-all` / `--connect <target>`: bring tunnels up once at start.
/// Failures are logged, never fatal — a forward whose bastion is down must not
/// stop the daemon (auto-reconnect keeps retrying per the settings).
async fn connect_on_start(state: &Arc<AppState>, options: &DaemonOptions) {
    if options.connect_all {
        if let Err(e) = run_start_all(state).await {
            tracing::error!(error = %e, "connect-all at start failed");
        }
    }
    for target in &options.connect {
        match crate::cli::service::resolve_target(state, target) {
            Ok(cfg) => {
                if let Err(e) = crate::ssh::engine::connect_forward(state, &cfg.id).await {
                    tracing::error!(tunnel = %cfg.name, error = %e, "connect at start failed");
                }
            }
            Err(e) => tracing::error!(target = %target, error = %e, "--connect target not found"),
        }
    }
}

/// Resolve on SIGINT (Ctrl-C) or SIGTERM (systemd stop / launchd unload).
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "cannot listen for SIGTERM; only Ctrl-C stops the daemon");
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}
