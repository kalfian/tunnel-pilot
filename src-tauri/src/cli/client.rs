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

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cli::args::{self, Command, Invocation};
use crate::cli::protocol::{
    status_label, BulkResult, CliCommand, CliData, CliRequest, CliResponse, ForwardView,
};
use crate::cli::shim::{self, CliShimStatus, ShimState};
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

/// Printed when a subcommand hides behind leading options — `tunnel-pilot
/// --json list` would otherwise launch the GUI and silently ignore the command.
pub const OPTIONS_AFTER_SUBCOMMAND_HINT: &str =
    "options go after the subcommand (e.g. 'tunnel-pilot list --json')";

/// Inspect argv; `Some(exit_code)` when this was a CLI invocation, `None` when
/// the caller should start the GUI instead.
///
/// Subcommands are positional words, so a bare launch and GUI flags
/// (`--minimized`, macOS's `-psn_*`) fall through untouched. A leading option
/// FOLLOWED by a subcommand is neither: it is a mistyped CLI call, and starting
/// a desktop app instead of running the command would be the worst answer.
pub fn dispatch(argv: &[String]) -> Option<i32> {
    let first = argv.get(1)?;
    if args::is_cli_subcommand(first) {
        return Some(run(&argv[1..]));
    }
    if first.starts_with('-') && argv[2..].iter().any(|a| args::is_cli_subcommand(a)) {
        eprintln!("error: {OPTIONS_AFTER_SUBCOMMAND_HINT}");
        eprintln!("run 'tunnel-pilot help' for usage");
        return Some(EXIT_USAGE);
    }
    None
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

    // `install-cli`/`uninstall-cli` are LOCAL: they operate on the filesystem,
    // so they must work with the app closed (no socket, never exit 3).
    match &invocation.command {
        Command::InstallCli { target } => return run_shim(&invocation, shim::install(*target)),
        Command::UninstallCli => return run_shim(&invocation, shim::uninstall()),
        _ => {}
    }

    let Some(path) = socket_path(&invocation) else {
        eprintln!("error: could not resolve the control socket path; set {SOCKET_ENV}");
        return EXIT_INTERNAL;
    };

    let request = build_request(&invocation.command);
    let budget = read_timeout(&invocation.command);
    let response = match send(&path, &request, budget) {
        Ok(res) => res,
        Err(e) => return report_client_error(&path, budget, e),
    };

    report(&invocation, response)
}

/// Print a transport failure and pick its exit code. A read timeout is a
/// TIMEOUT (6), not an internal error: the app is running and answering the
/// socket, it just did not finish in the budget the caller asked for.
fn report_client_error(path: &Path, budget: Duration, error: ClientError) -> i32 {
    match error {
        ClientError::NotRunning => {
            eprintln!("{NOT_RUNNING_HINT}");
            EXIT_NOT_RUNNING
        }
        ClientError::TimedOut => {
            eprintln!(
                "error: Tunnel Pilot did not answer within {}s on {}",
                budget.as_secs(),
                path.display()
            );
            EXIT_TIMEOUT
        }
        ClientError::Io(msg) => {
            eprintln!("error: control socket at {}: {msg}", path.display());
            EXIT_INTERNAL
        }
        ClientError::Protocol(msg) => {
            eprintln!("error: unexpected response from Tunnel Pilot: {msg}");
            EXIT_INTERNAL
        }
    }
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
        // Handled before we ever build a request (help prints, the shim
        // subcommands run locally) — no socket round-trip exists for them.
        Command::Help | Command::InstallCli { .. } | Command::UninstallCli => {
            CliRequest::new(CliCommand::Version)
        }
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
            force,
        } => CliRequest {
            target: Some(target.clone()),
            timeout_ms: Some(timeout_secs * 1_000),
            wait: *wait,
            force: *force,
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
#[derive(Debug, PartialEq, Eq)]
enum ClientError {
    /// No socket / nothing listening ⇒ the app is not running.
    NotRunning,
    /// The read timeout elapsed before the response line arrived.
    TimedOut,
    Io(String),
    Protocol(String),
}

/// Classify a socket READ failure. `set_read_timeout` surfaces as `WouldBlock`
/// on Linux and `TimedOut` on macOS — both mean "the budget elapsed", never an
/// internal fault.
fn classify_read_error(error: &std::io::Error) -> ClientError {
    match error.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => ClientError::TimedOut,
        _ => ClientError::Io(error.to_string()),
    }
}

#[cfg(unix)]
fn send(
    path: &Path,
    request: &CliRequest,
    read_timeout: Duration,
) -> Result<CliResponse, ClientError> {
    // Scoped to this fn: the `#[cfg(not(unix))]` `send` below uses none of
    // these, and a file-level import would be an unused-import error there
    // under `clippy -D warnings` (which is what the Windows CI job runs).
    use std::io::{BufRead, BufReader, Write};
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
        .map_err(|e| classify_read_error(&e))?;
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
    exit_code_for_data(&invocation.command, &data)
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

/// Exit code for a successful round-trip, derived from the payload AND the
/// command that produced it: a connect that ended in `error` is a failed
/// operation even though the query worked. `status`/`list`/`version` always exit
/// 0 — the state lives in the payload.
///
/// The command matters because `CliData::Action` is shared by `connect` and
/// `disconnect` (the payload is `untagged`, so a dedicated variant would risk
/// mis-deserialization): `disconnected` is success for one and failure for the
/// other. So when a WAIT was requested, any connect verb that did not end
/// `connected` — including a `Vanished` wait, which reports `disconnected` —
/// exits 5. Under `--no-wait`, `connecting` stays neutral: the caller asked not
/// to find out.
pub fn exit_code_for_data(command: &Command, data: &CliData) -> i32 {
    let awaited_connect = matches!(
        command,
        Command::Connect { wait: true, .. } | Command::ConnectAll { wait: true, .. }
    );
    match data {
        CliData::List { .. } | CliData::Status { .. } | CliData::Version { .. } => EXIT_OK,
        CliData::Action {
            forward, timed_out, ..
        } => {
            let failed = forward.status == ForwardStatus::Error
                || (awaited_connect && forward.status != ForwardStatus::Connected);
            if *timed_out {
                EXIT_TIMEOUT
            } else if failed {
                EXIT_OPERATION_FAILED
            } else {
                EXIT_OK
            }
        }
        CliData::Bulk {
            results, failed, ..
        } => {
            let any_failed = *failed > 0
                || (awaited_connect
                    && results.iter().any(|r| r.status != ForwardStatus::Connected));
            if results.iter().any(|r| r.timed_out) {
                EXIT_TIMEOUT
            } else if any_failed {
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

/// Print the outcome of a local shim command and pick its exit code.
///
/// No new exit code: a filesystem/permission failure is an I/O error (1) and a
/// refusal — dev build, or something that is not our symlink in the way — is a
/// usage error (2). Exit 3 can never happen here; the app need not be running.
fn run_shim(invocation: &Invocation, result: Result<CliShimStatus, AppError>) -> i32 {
    match result {
        Ok(status) => {
            if invocation.json {
                match serde_json::to_string(&status) {
                    Ok(json) => println!("{json}"),
                    Err(e) => {
                        eprintln!("error: {e}");
                        return EXIT_INTERNAL;
                    }
                }
            } else {
                print!("{}", render_shim_status(&status));
            }
            EXIT_OK
        }
        Err(e) => {
            eprintln!("error: {e}");
            exit_code_for_shim_error(&e)
        }
    }
}

/// A refusal we decided on (`InvalidInput`) is a usage error; anything else is
/// an I/O/internal failure.
pub fn exit_code_for_shim_error(error: &AppError) -> i32 {
    match error {
        AppError::InvalidInput(_) => EXIT_USAGE,
        _ => EXIT_INTERNAL,
    }
}

/// Field-per-line, same shape as `status` — an agent can grep it and a human
/// can read where the shim went.
pub fn render_shim_status(status: &CliShimStatus) -> String {
    let mut out = String::new();
    let mut field = |label: &str, value: String| {
        out.push_str(&format!("{}{}\n", pad(label, 12), value));
    };
    if !status.supported {
        field("supported", "no (macOS and Linux only)".into());
        return out;
    }
    field(
        "installed",
        match (status.installed, status.links_to_current) {
            (false, _) => "no".into(),
            (true, true) => "yes".into(),
            (true, false) => "yes (needs attention)".into(),
        },
    );
    if let Some(path) = &status.path {
        field("path", path.clone());
    }
    if let Some(linked) = &status.linked_path {
        field("links to", linked.clone());
    }
    if let Some(exe) = &status.current_exe {
        field("this app", exe.clone());
    }
    if status.conflict {
        field(
            "warning",
            "a real file is in the way; remove it yourself".into(),
        );
    } else if status.linked_elsewhere {
        field(
            "warning",
            "the shim points at another binary; re-run install-cli".into(),
        );
    }
    for entry in &status.entries {
        if Some(entry.path.as_str()) == status.path.as_deref() {
            continue; // already reported above
        }
        if entry.state != ShimState::Absent {
            field("also at", format!("{} ({:?})", entry.path, entry.state));
        }
    }
    if !status.installed {
        if let Some(install_path) = &status.install_path {
            field(
                "would use",
                format!(
                    "{install_path}{}",
                    if status.needs_elevation {
                        " (needs administrator rights)"
                    } else {
                        ""
                    }
                ),
            );
        }
    }
    if status.dev_build {
        field("note", "development build — install is refused".into());
    }
    out
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

    /// `connect <target>` with the default flags (waiting).
    fn connect_cmd(wait: bool) -> Command {
        Command::Connect {
            target: "prod-db".into(),
            timeout_secs: 30,
            wait,
            force: false,
        }
    }

    #[test]
    fn dispatch_ignores_gui_launches() {
        assert_eq!(dispatch(&strings(&["tunnel-pilot"])), None);
        assert_eq!(dispatch(&strings(&["tunnel-pilot", "--minimized"])), None);
        // macOS hands a process-serial-number flag to a Finder launch.
        assert_eq!(dispatch(&strings(&["tunnel-pilot", "-psn_0_1234"])), None);
    }

    /// A subcommand hidden behind leading options is a mistyped CLI call, not a
    /// GUI launch — launching the app would silently ignore what was asked.
    #[test]
    fn dispatch_rejects_options_before_the_subcommand() {
        for argv in [
            vec!["tunnel-pilot", "--json", "list"],
            vec![
                "tunnel-pilot",
                "--socket",
                "/tmp/x.sock",
                "status",
                "prod-db",
            ],
            vec!["tunnel-pilot", "--no-wait", "connect-all"],
        ] {
            assert_eq!(
                dispatch(&strings(&argv)),
                Some(EXIT_USAGE),
                "{argv:?} must not launch the GUI"
            );
        }
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
        let connect = connect_cmd(true);
        let ok = CliData::Action {
            forward: view("id", "n", ForwardStatus::Connected, None),
            waited: true,
            timed_out: false,
        };
        assert_eq!(exit_code_for_data(&connect, &ok), EXIT_OK);

        let failed = CliData::Action {
            forward: view("id", "n", ForwardStatus::Error, Some("auth failed")),
            waited: true,
            timed_out: false,
        };
        assert_eq!(exit_code_for_data(&connect, &failed), EXIT_OPERATION_FAILED);

        let timed_out = CliData::Action {
            forward: view("id", "n", ForwardStatus::Connecting, None),
            waited: true,
            timed_out: true,
        };
        assert_eq!(exit_code_for_data(&connect, &timed_out), EXIT_TIMEOUT);

        // A `status` query succeeds even when the tunnel is in error.
        let status = CliData::Status {
            forward: view("id", "n", ForwardStatus::Error, Some("auth failed")),
            stats: TunnelStats::default(),
        };
        assert_eq!(exit_code_for_data(&Command::List, &status), EXIT_OK);

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
        assert_eq!(
            exit_code_for_data(&Command::DisconnectAll, &bulk_failed),
            EXIT_OPERATION_FAILED
        );

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
        assert_eq!(
            exit_code_for_data(&Command::DisconnectAll, &bulk_timed_out),
            EXIT_TIMEOUT
        );
    }

    /// Finding 4: a waited `connect` whose tunnel ended `disconnected` (the
    /// `Vanished` wait outcome) must NOT look like success. The same payload
    /// from `disconnect` is success — hence the branch on the command.
    #[test]
    fn a_waited_connect_that_did_not_connect_exits_five() {
        let vanished = CliData::Action {
            forward: view("id", "n", ForwardStatus::Disconnected, None),
            waited: true,
            timed_out: false,
        };
        assert_eq!(
            exit_code_for_data(&connect_cmd(true), &vanished),
            EXIT_OPERATION_FAILED
        );
        assert_eq!(
            exit_code_for_data(
                &Command::ConnectAll {
                    timeout_secs: 30,
                    wait: true
                },
                &CliData::Bulk {
                    results: vec![BulkResult {
                        id: "id".into(),
                        name: "n".into(),
                        status: ForwardStatus::Disconnected,
                        last_error: None,
                        timed_out: false,
                    }],
                    succeeded: 0,
                    failed: 0,
                }
            ),
            EXIT_OPERATION_FAILED
        );

        // `disconnect` producing the very same payload is a success.
        assert_eq!(
            exit_code_for_data(&Command::Disconnect { target: "n".into() }, &vanished),
            EXIT_OK
        );

        // `--no-wait`: an unfinished connect stays neutral.
        let dispatched = CliData::Action {
            forward: view("id", "n", ForwardStatus::Connecting, None),
            waited: false,
            timed_out: false,
        };
        assert_eq!(
            exit_code_for_data(&connect_cmd(false), &dispatched),
            EXIT_OK
        );
    }

    #[test]
    fn requests_carry_the_flags_the_user_typed() {
        let req = build_request(&Command::Connect {
            target: "prod-db".into(),
            timeout_secs: 45,
            wait: true,
            force: true,
        });
        assert_eq!(req.cmd, CliCommand::Connect);
        assert_eq!(req.target.as_deref(), Some("prod-db"));
        assert_eq!(req.timeout_ms, Some(45_000));
        assert!(req.wait);
        assert!(req.force, "--force must reach the server");
        assert!(
            !build_request(&connect_cmd(true)).force,
            "connect is idempotent unless --force"
        );

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
            force: false,
        });
        assert!(waiting > Duration::from_secs(300), "{waiting:?}");

        let not_waiting = read_timeout(&Command::Connect {
            target: "x".into(),
            timeout_secs: 300,
            wait: false,
            force: false,
        });
        assert_eq!(not_waiting, DEFAULT_READ_TIMEOUT);
    }

    /// A server that accepts but never answers must exit 6 (timed out), not 1:
    /// the app IS running, it just did not finish inside the budget.
    #[cfg(unix)]
    #[test]
    fn a_stalled_server_times_out_instead_of_looking_internal() {
        use std::os::unix::net::UnixListener;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("stalled.sock");
        let listener = UnixListener::bind(&path).expect("bind");
        // Accept the connection, read nothing, answer nothing.
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            std::thread::sleep(Duration::from_millis(400));
            drop(stream);
        });

        let error = send(
            &path,
            &CliRequest::new(CliCommand::List),
            Duration::from_millis(100),
        )
        .expect_err("a stalled server must not answer");
        assert_eq!(error, ClientError::TimedOut, "{error:?}");
        assert_eq!(
            report_client_error(&path, Duration::from_millis(100), error),
            EXIT_TIMEOUT
        );
        server.join().expect("server thread");
    }

    #[test]
    fn transport_failures_map_to_the_documented_exit_codes() {
        let path = Path::new("/tmp/does-not-matter.sock");
        let budget = Duration::from_secs(1);
        assert_eq!(
            report_client_error(path, budget, ClientError::NotRunning),
            EXIT_NOT_RUNNING
        );
        assert_eq!(
            report_client_error(path, budget, ClientError::TimedOut),
            EXIT_TIMEOUT
        );
        assert_eq!(
            report_client_error(path, budget, ClientError::Io("broken pipe".into())),
            EXIT_INTERNAL
        );
        assert_eq!(
            report_client_error(path, budget, ClientError::Protocol("garbage".into())),
            EXIT_INTERNAL
        );
    }

    /// `set_read_timeout` surfaces as `WouldBlock` on Linux, `TimedOut` on macOS.
    #[test]
    fn both_read_timeout_error_kinds_are_recognized() {
        use std::io::{Error, ErrorKind};
        for kind in [ErrorKind::WouldBlock, ErrorKind::TimedOut] {
            assert_eq!(
                classify_read_error(&Error::new(kind, "timed out")),
                ClientError::TimedOut,
                "{kind:?}"
            );
        }
        assert!(matches!(
            classify_read_error(&Error::new(ErrorKind::BrokenPipe, "gone")),
            ClientError::Io(_)
        ));
    }

    #[test]
    fn byte_formatting_is_ascii_and_stable() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MB");
    }

    fn shim_status_fixture() -> CliShimStatus {
        CliShimStatus {
            supported: true,
            installed: true,
            path: Some("/Users/me/.local/bin/tunnel-pilot".into()),
            target: Some(crate::cli::shim::ShimTarget::UserLocal),
            links_to_current: true,
            linked_path: Some("/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot".into()),
            linked_elsewhere: false,
            conflict: false,
            install_target: Some(crate::cli::shim::ShimTarget::UserLocal),
            install_path: Some("/Users/me/.local/bin/tunnel-pilot".into()),
            needs_elevation: false,
            current_exe: Some("/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot".into()),
            dev_build: false,
            entries: Vec::new(),
        }
    }

    #[test]
    fn shim_rendering_states_the_path_and_the_problem() {
        let out = render_shim_status(&shim_status_fixture());
        assert!(out.contains("installed   yes"), "{out}");
        assert!(out.contains("/Users/me/.local/bin/tunnel-pilot"), "{out}");
        assert!(
            !out.contains("warning"),
            "a healthy shim has no warning: {out}"
        );

        let stale = CliShimStatus {
            links_to_current: false,
            linked_elsewhere: true,
            ..shim_status_fixture()
        };
        let out = render_shim_status(&stale);
        assert!(out.contains("needs attention"), "{out}");
        assert!(out.contains("re-run install-cli"), "{out}");

        let absent = CliShimStatus {
            installed: false,
            links_to_current: false,
            path: None,
            linked_path: None,
            target: None,
            needs_elevation: true,
            install_path: Some("/usr/local/bin/tunnel-pilot".into()),
            ..shim_status_fixture()
        };
        let out = render_shim_status(&absent);
        assert!(out.contains("installed   no"), "{out}");
        assert!(out.contains("administrator rights"), "{out}");

        let unsupported = render_shim_status(&CliShimStatus::unsupported());
        assert!(
            unsupported.contains("macOS and Linux only"),
            "{unsupported}"
        );
    }

    /// The local shim commands reuse the documented codes: a refusal is a usage
    /// error, everything else is an I/O failure. Exit 3 is impossible — they do
    /// not need the app to be running.
    #[test]
    fn shim_errors_map_onto_the_existing_exit_codes() {
        assert_eq!(
            exit_code_for_shim_error(&AppError::InvalidInput("dev build".into())),
            EXIT_USAGE
        );
        assert_eq!(
            exit_code_for_shim_error(&AppError::Io("permission denied".into())),
            EXIT_INTERNAL
        );
        assert_eq!(
            exit_code_for_shim_error(&AppError::Internal("boom".into())),
            EXIT_INTERNAL
        );
    }
}
