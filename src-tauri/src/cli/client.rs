//! The CLI client: the same binary, invoked with a subcommand (spec 03 §20).
//!
//! `main.rs` calls [`dispatch`] before any Tauri code. When argv[1] is a known
//! subcommand this runs a blocking `std::os::unix::net::UnixStream` round-trip
//! and exits with a meaningful code; otherwise it returns `None` and the GUI
//! starts as usual (`--minimized` is a flag, never a subcommand).
//!
//! No tokio here on purpose: a CLI invocation should not pay for a runtime.
//! Rendering is plain ASCII with fixed-width columns — no ANSI colour, full
//! ids — so both a human and an agent can read it, and `--json` prints the
//! response payload verbatim.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::args::{self, Command, Invocation};
use crate::cli::protocol::{
    status_label, BulkResult, CliCommand, CliData, CliRequest, CliResponse, ForwardView,
};
use crate::cli::SOCKET_ENV;
use crate::error::AppError;
use crate::state::models::{ForwardStatus, TunnelStats};

/// Success.
pub const EXIT_OK: i32 = 0;
/// Internal / IO / protocol failure.
pub const EXIT_INTERNAL: i32 = 1;
/// Usage error (unknown subcommand, missing argument, bad flag).
pub const EXIT_USAGE: i32 = 2;
/// The app is not running (socket missing or refusing connections).
pub const EXIT_NOT_RUNNING: i32 = 3;
/// Target not found, or ambiguous.
pub const EXIT_NOT_FOUND: i32 = 4;
/// The operation itself ended in `error`.
pub const EXIT_OPERATION_FAILED: i32 = 5;
/// Timed out waiting for a terminal status.
pub const EXIT_TIMEOUT: i32 = 6;

/// Printed when the socket is absent. We never auto-launch the GUI: an agent
/// must not silently start a desktop process.
pub const NOT_RUNNING_HINT: &str =
    "Tunnel Pilot is not running. Start it with: open -a \"Tunnel Pilot\"";

/// Slack added to the wait budget when computing the socket read timeout, so
/// the server's own answer always wins the race against our timeout.
const READ_TIMEOUT_SLACK: Duration = Duration::from_secs(10);
/// Read timeout for commands that do not wait on a tunnel.
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Inspect argv; `Some(exit_code)` when this was a CLI invocation, `None` when
/// the caller should start the GUI instead.
pub fn dispatch(argv: &[String]) -> Option<i32> {
    let first = argv.get(1)?;
    if !args::is_cli_subcommand(first) {
        return None;
    }
    Some(run(&argv[1..]))
}

/// Run one CLI invocation (argv tail) and return its exit code.
pub fn run(args: &[String]) -> i32 {
    let invocation = match args::parse(args) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("run 'tunnel-pilot help' for usage");
            return EXIT_USAGE;
        }
    };

    if invocation.command == Command::Help {
        print!("{}", args::HELP_TEXT);
        return EXIT_OK;
    }

    let Some(path) = socket_path(&invocation) else {
        eprintln!("error: could not resolve the control socket path; set {SOCKET_ENV}");
        return EXIT_INTERNAL;
    };

    let request = build_request(&invocation.command);
    let response = match send(&path, &request, read_timeout(&invocation.command)) {
        Ok(res) => res,
        Err(ClientError::NotRunning) => {
            eprintln!("{NOT_RUNNING_HINT}");
            return EXIT_NOT_RUNNING;
        }
        Err(ClientError::Io(msg)) => {
            eprintln!("error: control socket at {}: {msg}", path.display());
            return EXIT_INTERNAL;
        }
        Err(ClientError::Protocol(msg)) => {
            eprintln!("error: unexpected response from Tunnel Pilot: {msg}");
            return EXIT_INTERNAL;
        }
    };

    report(&invocation, response)
}

/// Explicit `--socket` beats `TUNNEL_PILOT_SOCKET`, which beats the default.
fn socket_path(invocation: &Invocation) -> Option<PathBuf> {
    invocation
        .socket
        .clone()
        .or_else(crate::cli::default_socket_path)
}

fn build_request(command: &Command) -> CliRequest {
    match command {
        Command::Help => CliRequest::new(CliCommand::Version), // unreachable (handled above)
        Command::List => CliRequest::new(CliCommand::List),
        Command::Version => CliRequest::new(CliCommand::Version),
        Command::Status { target } => CliRequest {
            target: Some(target.clone()),
            ..CliRequest::new(CliCommand::Status)
        },
        Command::Disconnect { target } => CliRequest {
            target: Some(target.clone()),
            ..CliRequest::new(CliCommand::Disconnect)
        },
        Command::Connect {
            target,
            timeout_secs,
            wait,
        } => CliRequest {
            target: Some(target.clone()),
            timeout_ms: Some(timeout_secs * 1_000),
            wait: *wait,
            ..CliRequest::new(CliCommand::Connect)
        },
        Command::ConnectAll { timeout_secs, wait } => CliRequest {
            timeout_ms: Some(timeout_secs * 1_000),
            wait: *wait,
            ..CliRequest::new(CliCommand::ConnectAll)
        },
        Command::DisconnectAll => CliRequest::new(CliCommand::DisconnectAll),
    }
}

/// How long to wait for the response line: the server's own budget plus slack,
/// so a `connect --timeout 300` is answered rather than cut off locally.
fn read_timeout(command: &Command) -> Duration {
    match command {
        Command::Connect {
            timeout_secs, wait, ..
        }
        | Command::ConnectAll { timeout_secs, wait }
            if *wait =>
        {
            Duration::from_secs(*timeout_secs) + READ_TIMEOUT_SLACK
        }
        _ => DEFAULT_READ_TIMEOUT,
    }
}

/// Transport-level failure (never an application error — those come back as a
/// `CliResponse` with `ok: false`).
#[derive(Debug)]
enum ClientError {
    /// No socket / nothing listening ⇒ the app is not running.
    NotRunning,
    Io(String),
    Protocol(String),
}

#[cfg(unix)]
fn send(
    path: &Path,
    request: &CliRequest,
    read_timeout: Duration,
) -> Result<CliResponse, ClientError> {
    use std::os::unix::net::UnixStream;

    let stream = UnixStream::connect(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
            ClientError::NotRunning
        }
        _ => ClientError::Io(e.to_string()),
    })?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|e| ClientError::Io(e.to_string()))?;

    let mut writer = stream
        .try_clone()
        .map_err(|e| ClientError::Io(e.to_string()))?;
    let line = serde_json::to_string(request).map_err(|e| ClientError::Protocol(e.to_string()))?;
    writer
        .write_all(format!("{line}\n").as_bytes())
        .and_then(|()| writer.flush())
        .map_err(|e| ClientError::Io(e.to_string()))?;

    let mut response = String::new();
    BufReader::new(stream)
        .read_line(&mut response)
        .map_err(|e| ClientError::Io(e.to_string()))?;
    if response.trim().is_empty() {
        return Err(ClientError::Protocol("empty response".into()));
    }
    serde_json::from_str(response.trim())
        .map_err(|e| ClientError::Protocol(format!("{e}: {}", response.trim())))
}

#[cfg(not(unix))]
fn send(
    _path: &Path,
    _request: &CliRequest,
    _read_timeout: Duration,
) -> Result<CliResponse, ClientError> {
    Err(ClientError::Io(
        "the control socket is only available on macOS and Linux".into(),
    ))
}

/// Print the response (human table or raw JSON) and pick the exit code.
fn report(invocation: &Invocation, response: CliResponse) -> i32 {
    if !response.ok {
        let error = response
            .error
            .unwrap_or_else(|| AppError::Internal("failed without an error payload".into()));
        if invocation.json {
            match serde_json::to_string(&error) {
                Ok(json) => eprintln!("{json}"),
                Err(e) => eprintln!("error: {e}"),
            }
        } else {
            eprintln!("error: {}", error_message(&error));
        }
        return exit_code_for_error(&error);
    }

    let Some(data) = response.data else {
        eprintln!("error: succeeded without a payload");
        return EXIT_INTERNAL;
    };

    if invocation.json {
        match serde_json::to_string(&data) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("error: {e}");
                return EXIT_INTERNAL;
            }
        }
    } else {
        print!("{}", render(&data));
    }
    exit_code_for_data(&data)
}

/// Human-readable rendering for each payload.
pub fn render(data: &CliData) -> String {
    match data {
        CliData::List { forwards } => render_list(forwards),
        CliData::Status { forward, stats } => render_status(forward, stats),
        CliData::Action {
            forward,
            waited,
            timed_out,
        } => render_action(forward, *waited, *timed_out),
        CliData::Bulk {
            results,
            succeeded,
            failed,
        } => render_bulk(results, *succeeded, *failed),
        CliData::Version {
            app_version,
            protocol,
        } => render_version(app_version, *protocol),
    }
}

/// Fixed-width columns, no colour, full ids — an agent can `awk` this and a
/// human can read it. A tunnel's last error is appended after the id.
pub fn render_list(forwards: &[ForwardView]) -> String {
    if forwards.is_empty() {
        return "No forwards configured.\n".to_string();
    }
    let mut rows: Vec<[String; 5]> = vec![[
        "STATUS".into(),
        "NAME".into(),
        "LOCAL".into(),
        "REMOTE".into(),
        "ID".into(),
    ]];
    let mut trailers: Vec<String> = vec![String::new()];
    for f in forwards {
        rows.push([
            status_label(f.status).to_string(),
            f.name.clone(),
            f.local_endpoint(),
            f.remote_endpoint(),
            f.id.clone(),
        ]);
        trailers.push(match (&f.last_error, f.status) {
            (Some(e), ForwardStatus::Error) => e.clone(),
            _ => String::new(),
        });
    }

    let mut widths = [0usize; 5];
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    let mut out = String::new();
    for (row, trailer) in rows.iter().zip(trailers.iter()) {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            line.push_str(&pad(cell, widths[i]));
            line.push_str("  ");
        }
        line.push_str(trailer);
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

pub fn render_status(forward: &ForwardView, stats: &TunnelStats) -> String {
    let mut out = String::new();
    let mut field = |label: &str, value: String| {
        out.push_str(&format!("{}{}\n", pad(label, 12), value));
    };
    field("name", forward.name.clone());
    field("id", forward.id.clone());
    field("status", status_label(forward.status).to_string());
    field("local", forward.local_endpoint());
    field("remote", forward.remote_endpoint());
    field(
        "ssh",
        format!(
            "{}@{}:{}",
            forward.ssh_username, forward.ssh_host, forward.ssh_port
        ),
    );
    if !forward.tags.is_empty() {
        field("tags", forward.tags.join(", "));
    }
    field("connections", stats.active_connections.to_string());
    field(
        "transfer",
        format!(
            "up {} / down {}",
            format_bytes(stats.total_bytes_up),
            format_bytes(stats.total_bytes_down)
        ),
    );
    if let Some(latency) = stats.last_ping_latency_ms {
        field("latency", format!("{latency} ms"));
    }
    if let Some(since) = &stats.connected_since {
        field("since", since.clone());
    }
    if let Some(error) = &forward.last_error {
        field("error", error.clone());
    }
    out
}

pub fn render_action(forward: &ForwardView, waited: bool, timed_out: bool) -> String {
    let suffix = if timed_out {
        " (timed out waiting for a terminal status)"
    } else if !waited {
        " (dispatched; not waited)"
    } else {
        ""
    };
    let error = match &forward.last_error {
        Some(e) if forward.status == ForwardStatus::Error => format!(": {e}"),
        _ => String::new(),
    };
    format!(
        "{}  {}{}{}\n",
        forward.name,
        status_label(forward.status),
        error,
        suffix
    )
}

pub fn render_bulk(results: &[BulkResult], succeeded: usize, failed: usize) -> String {
    if results.is_empty() {
        return "No forwards configured.\n".to_string();
    }
    let name_width = results
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for r in results {
        let mut line = format!("{}  {}", pad(&r.name, name_width), status_label(r.status));
        if r.timed_out {
            line.push_str(" (timed out)");
        }
        if let (Some(e), ForwardStatus::Error) = (&r.last_error, r.status) {
            line.push_str(&format!(": {e}"));
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str(&format!(
        "{} succeeded, {} failed, {} total\n",
        succeeded,
        failed,
        results.len()
    ));
    out
}

pub fn render_version(app_version: &str, protocol: u32) -> String {
    format!(
        "cli       {}\napp       {app_version}\nprotocol  {protocol}\n",
        env!("CARGO_PKG_VERSION")
    )
}

/// Exit code for a successful round-trip, derived from the payload: a connect
/// that ended in `error` is a failed operation even though the query worked.
/// `status`/`list`/`version` always exit 0 — the state lives in the payload.
pub fn exit_code_for_data(data: &CliData) -> i32 {
    match data {
        CliData::List { .. } | CliData::Status { .. } | CliData::Version { .. } => EXIT_OK,
        CliData::Action {
            forward, timed_out, ..
        } => {
            if *timed_out {
                EXIT_TIMEOUT
            } else if forward.status == ForwardStatus::Error {
                EXIT_OPERATION_FAILED
            } else {
                EXIT_OK
            }
        }
        CliData::Bulk {
            results, failed, ..
        } => {
            if results.iter().any(|r| r.timed_out) {
                EXIT_TIMEOUT
            } else if *failed > 0 {
                EXIT_OPERATION_FAILED
            } else {
                EXIT_OK
            }
        }
    }
}

/// Map the app's error vocabulary onto exit codes.
pub fn exit_code_for_error(error: &AppError) -> i32 {
    match error {
        // Resolution failures: not found, or ambiguous (an `invalidInput`).
        AppError::NotFound(_) | AppError::InvalidInput(_) => EXIT_NOT_FOUND,
        // The operation reached the engine and failed there.
        AppError::Ssh(_) | AppError::Connection(_) => EXIT_OPERATION_FAILED,
        _ => EXIT_INTERNAL,
    }
}

/// The message half of an `AppError` (its `Display` already prefixes a kind).
fn error_message(error: &AppError) -> String {
    error.to_string()
}

fn pad(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if len >= width {
        value.to_string()
    } else {
        format!("{value}{}", " ".repeat(width - len))
    }
}

/// Human byte sizes; ASCII only.
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::cli::protocol::sample_config;
    use crate::state::models::ForwardRuntime;

    fn view(id: &str, name: &str, status: ForwardStatus, last_error: Option<&str>) -> ForwardView {
        let mut cfg = sample_config(id, name);
        cfg.local_port = 5432;
        ForwardView::from_parts(
            &cfg,
            &ForwardRuntime {
                status,
                stats: TunnelStats::default(),
                last_error: last_error.map(|e| e.to_string()),
            },
        )
    }

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn dispatch_ignores_gui_launches() {
        assert_eq!(dispatch(&strings(&["tunnel-pilot"])), None);
        assert_eq!(dispatch(&strings(&["tunnel-pilot", "--minimized"])), None);
    }

    #[test]
    fn dispatch_handles_help_locally_without_a_socket() {
        assert_eq!(dispatch(&strings(&["tunnel-pilot", "help"])), Some(EXIT_OK));
        assert_eq!(
            dispatch(&strings(&["tunnel-pilot", "--help"])),
            Some(EXIT_OK)
        );
    }

    #[test]
    fn a_usage_error_exits_two() {
        assert_eq!(run(&strings(&["status"])), EXIT_USAGE);
        assert_eq!(run(&strings(&["list", "--nope"])), EXIT_USAGE);
    }

    #[test]
    fn a_missing_socket_reports_the_app_is_not_running() {
        let path = std::env::temp_dir().join("tunnel-pilot-test-does-not-exist.sock");
        let _ = std::fs::remove_file(&path);
        let code = run(&strings(&[
            "list",
            "--socket",
            path.to_str().expect("utf-8 temp path"),
        ]));
        assert_eq!(code, EXIT_NOT_RUNNING);
        assert!(
            NOT_RUNNING_HINT.contains("open -a"),
            "hint must be actionable"
        );
    }

    #[test]
    fn list_columns_line_up_and_ids_are_printed_in_full() {
        let out = render_list(&[
            view(
                "9f3c1a02-0000-4000-8000-000000000001",
                "prod-db",
                ForwardStatus::Connected,
                None,
            ),
            view(
                "1b77e5d4-0000-4000-8000-000000000002",
                "staging-redis-longer",
                ForwardStatus::Disconnected,
                None,
            ),
            view(
                "c0ffee12-0000-4000-8000-000000000003",
                "legacy-api",
                ForwardStatus::Error,
                Some("auth failed"),
            ),
        ]);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 4, "header + 3 rows:\n{out}");
        assert!(lines[0].starts_with("STATUS"), "{}", lines[0]);

        // Every row's NAME cell starts at the header's NAME offset — i.e. the
        // columns line up rather than merely being space-separated.
        let name_col = lines[0].find("NAME").expect("NAME header");
        for (line, expected) in
            lines[1..]
                .iter()
                .zip(["prod-db", "staging-redis-longer", "legacy-api"])
        {
            assert!(
                line[name_col..].starts_with(expected),
                "column drift at {name_col} in: {line}"
            );
        }
        // Full uuid, and the error trails the id.
        assert!(out.contains("9f3c1a02-0000-4000-8000-000000000001"));
        assert!(lines[3].ends_with("auth failed"), "{}", lines[3]);
        // No ANSI escapes.
        assert!(!out.contains('\u{1b}'), "output must be plain text");
    }

    #[test]
    fn an_empty_list_says_so() {
        assert_eq!(render_list(&[]), "No forwards configured.\n");
    }

    #[test]
    fn status_renders_endpoints_stats_and_error() {
        let stats = TunnelStats {
            active_connections: 2,
            total_bytes_up: 2048,
            total_bytes_down: 512,
            last_ping_latency_ms: Some(17),
            connected_since: Some("2026-09-10T09:00:00Z".into()),
        };
        let out = render_status(
            &view("id-1", "prod-db", ForwardStatus::Error, Some("auth failed")),
            &stats,
        );
        assert!(out.contains("status      error"), "{out}");
        assert!(out.contains("local       127.0.0.1:5432"), "{out}");
        assert!(out.contains("remote      db.internal:5432"), "{out}");
        assert!(
            out.contains("ssh         deploy@bastion.example.com:22"),
            "{out}"
        );
        assert!(out.contains("up 2.0 KB / down 512 B"), "{out}");
        assert!(out.contains("latency     17 ms"), "{out}");
        assert!(out.contains("error       auth failed"), "{out}");
    }

    #[test]
    fn action_lines_report_wait_and_failure_shape() {
        let connected = render_action(
            &view("id", "prod-db", ForwardStatus::Connected, None),
            true,
            false,
        );
        assert_eq!(connected, "prod-db  connected\n");

        let failed = render_action(
            &view("id", "prod-db", ForwardStatus::Error, Some("auth failed")),
            true,
            false,
        );
        assert_eq!(failed, "prod-db  error: auth failed\n");

        let dispatched = render_action(
            &view("id", "prod-db", ForwardStatus::Connecting, None),
            false,
            false,
        );
        assert!(dispatched.contains("not waited"), "{dispatched}");

        let timed_out = render_action(
            &view("id", "prod-db", ForwardStatus::Connecting, None),
            true,
            true,
        );
        assert!(timed_out.contains("timed out"), "{timed_out}");
    }

    #[test]
    fn bulk_output_summarizes_the_sweep() {
        let results = vec![
            BulkResult {
                id: "id-a".into(),
                name: "prod-db".into(),
                status: ForwardStatus::Connected,
                last_error: None,
                timed_out: false,
            },
            BulkResult {
                id: "id-b".into(),
                name: "legacy".into(),
                status: ForwardStatus::Error,
                last_error: Some("auth failed".into()),
                timed_out: false,
            },
        ];
        let out = render_bulk(&results, 1, 1);
        assert!(out.contains("prod-db  connected"), "{out}");
        assert!(out.contains("legacy   error: auth failed"), "{out}");
        assert!(out.contains("1 succeeded, 1 failed, 2 total"), "{out}");
    }

    #[test]
    fn version_output_separates_the_cli_from_the_app() {
        let out = render_version("2.1.0", 1);
        assert!(
            out.contains(&format!("cli       {}", env!("CARGO_PKG_VERSION"))),
            "{out}"
        );
        assert!(out.contains("app       2.1.0"), "{out}");
        assert!(out.contains("protocol  1"), "{out}");
    }

    #[test]
    fn error_kinds_map_to_the_documented_exit_codes() {
        let cases = [
            (AppError::NotFound("forward x".into()), EXIT_NOT_FOUND),
            (AppError::InvalidInput("ambiguous".into()), EXIT_NOT_FOUND),
            (AppError::Ssh("auth failed".into()), EXIT_OPERATION_FAILED),
            (
                AppError::Connection("bind failed".into()),
                EXIT_OPERATION_FAILED,
            ),
            (AppError::Storage("disk".into()), EXIT_INTERNAL),
            (AppError::Internal("boom".into()), EXIT_INTERNAL),
        ];
        for (error, expected) in cases {
            assert_eq!(exit_code_for_error(&error), expected, "{error:?}");
        }
    }

    #[test]
    fn payload_outcomes_map_to_the_documented_exit_codes() {
        let ok = CliData::Action {
            forward: view("id", "n", ForwardStatus::Connected, None),
            waited: true,
            timed_out: false,
        };
        assert_eq!(exit_code_for_data(&ok), EXIT_OK);

        let failed = CliData::Action {
            forward: view("id", "n", ForwardStatus::Error, Some("auth failed")),
            waited: true,
            timed_out: false,
        };
        assert_eq!(exit_code_for_data(&failed), EXIT_OPERATION_FAILED);

        let timed_out = CliData::Action {
            forward: view("id", "n", ForwardStatus::Connecting, None),
            waited: true,
            timed_out: true,
        };
        assert_eq!(exit_code_for_data(&timed_out), EXIT_TIMEOUT);

        // A `status` query succeeds even when the tunnel is in error.
        let status = CliData::Status {
            forward: view("id", "n", ForwardStatus::Error, Some("auth failed")),
            stats: TunnelStats::default(),
        };
        assert_eq!(exit_code_for_data(&status), EXIT_OK);

        let bulk_failed = CliData::Bulk {
            results: vec![BulkResult {
                id: "id".into(),
                name: "n".into(),
                status: ForwardStatus::Error,
                last_error: None,
                timed_out: false,
            }],
            succeeded: 0,
            failed: 1,
        };
        assert_eq!(exit_code_for_data(&bulk_failed), EXIT_OPERATION_FAILED);

        let bulk_timed_out = CliData::Bulk {
            results: vec![BulkResult {
                id: "id".into(),
                name: "n".into(),
                status: ForwardStatus::Connecting,
                last_error: None,
                timed_out: true,
            }],
            succeeded: 0,
            failed: 1,
        };
        assert_eq!(exit_code_for_data(&bulk_timed_out), EXIT_TIMEOUT);
    }

    #[test]
    fn requests_carry_the_flags_the_user_typed() {
        let req = build_request(&Command::Connect {
            target: "prod-db".into(),
            timeout_secs: 45,
            wait: true,
        });
        assert_eq!(req.cmd, CliCommand::Connect);
        assert_eq!(req.target.as_deref(), Some("prod-db"));
        assert_eq!(req.timeout_ms, Some(45_000));
        assert!(req.wait);

        let req = build_request(&Command::ConnectAll {
            timeout_secs: 30,
            wait: false,
        });
        assert_eq!(req.cmd, CliCommand::ConnectAll);
        assert!(!req.wait);
        assert!(req.target.is_none());
    }

    #[test]
    fn the_read_timeout_covers_the_servers_own_budget() {
        let waiting = read_timeout(&Command::Connect {
            target: "x".into(),
            timeout_secs: 300,
            wait: true,
        });
        assert!(waiting > Duration::from_secs(300), "{waiting:?}");

        let not_waiting = read_timeout(&Command::Connect {
            target: "x".into(),
            timeout_secs: 300,
            wait: false,
        });
        assert_eq!(not_waiting, DEFAULT_READ_TIMEOUT);
    }

    #[test]
    fn byte_formatting_is_ascii_and_stable() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }
}
