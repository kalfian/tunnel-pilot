//! Hand-rolled argument parsing for the CLI (spec 03 §20).
//!
//! Deliberately no `clap`: the surface is eight fixed subcommands with four
//! flags, and the GUI binary should not carry an argument-parsing framework it
//! never uses. Revisit if the surface grows.
//!
//! [`parse`] is pure — it takes the argv tail and returns either an
//! [`Invocation`] or a [`UsageError`], so every branch is unit-testable without
//! a process or a socket.

use std::path::PathBuf;

use crate::cli::protocol::{MAX_TIMEOUT_MS, MIN_TIMEOUT_MS};

/// Lower/upper bounds for `--timeout`, in seconds (mirrors the server clamp).
pub const MIN_TIMEOUT_SECS: u64 = MIN_TIMEOUT_MS / 1_000;
pub const MAX_TIMEOUT_SECS: u64 = MAX_TIMEOUT_MS / 1_000;
/// Default wait budget in seconds when `--timeout` is absent.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// A fully parsed invocation: what to do, plus the global flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub command: Command,
    /// `--json`: print the response `data` object verbatim instead of a table.
    pub json: bool,
    /// `--socket <path>`: override the socket path (beats `TUNNEL_PILOT_SOCKET`).
    pub socket: Option<PathBuf>,
}

/// The subcommand plus its own arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Help,
    List,
    Status {
        target: String,
    },
    Connect {
        target: String,
        timeout_secs: u64,
        wait: bool,
    },
    Disconnect {
        target: String,
    },
    ConnectAll {
        timeout_secs: u64,
        wait: bool,
    },
    DisconnectAll,
    Version,
}

/// A user-facing usage failure → exit code 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageError {
    pub message: String,
}

impl UsageError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for UsageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// Whether `word` starts a CLI invocation. Used by `main.rs` to decide between
/// the CLI client and launching the GUI — it must NEVER match a GUI flag such
/// as `--minimized` (subcommands are positional words).
pub fn is_cli_subcommand(word: &str) -> bool {
    matches!(
        word,
        "list"
            | "status"
            | "connect"
            | "disconnect"
            | "connect-all"
            | "disconnect-all"
            | "version"
            | "help"
            | "--help"
            | "-h"
    )
}

pub const HELP_TEXT: &str = "\
tunnel-pilot — control the running Tunnel Pilot app from a terminal

USAGE:
    tunnel-pilot <command> [options]

COMMANDS:
    list                       List every configured forward and its status
    status <id|name>           Show one forward's status, endpoints and stats
    connect <id|name>          Connect a forward and wait for a terminal status
    disconnect <id|name>       Disconnect a forward (waits for teardown)
    connect-all                Connect every forward (tray 'Start All')
    disconnect-all             Disconnect every live forward (tray 'Stop All')
    version                    App version + protocol version
    help                       Show this help

OPTIONS:
    --json                     Print the raw JSON response payload
    --socket <path>            Control socket path (default: app config dir)
    --timeout <secs>           Wait budget for connect/connect-all (1-300, default 30)
    --no-wait                  Return as soon as the connect is dispatched

TARGETS:
    A target is a forward id (exact) or name (case-insensitive, exact).
    Ambiguous names are rejected — use the id.

EXIT CODES:
    0 ok   1 internal/IO   2 usage   3 app not running
    4 target not found or ambiguous   5 operation ended in error   6 timed out

ENVIRONMENT:
    TUNNEL_PILOT_SOCKET        Socket path override (both app and CLI)
";

/// Parse the argv tail (everything after the program name).
pub fn parse(args: &[String]) -> Result<Invocation, UsageError> {
    let Some(subcommand) = args.first() else {
        return Err(UsageError::new("missing command"));
    };

    let mut json = false;
    let mut socket: Option<PathBuf> = None;
    let mut timeout_secs: Option<u64> = None;
    let mut wait = true;
    let mut positionals: Vec<&str> = Vec::new();

    let mut rest = args[1..].iter().map(|s| s.as_str()).peekable();
    while let Some(arg) = rest.next() {
        match arg {
            "--json" => json = true,
            "--no-wait" => wait = false,
            "--help" | "-h" => {
                return Ok(Invocation {
                    command: Command::Help,
                    json,
                    socket,
                })
            }
            _ if arg == "--socket" || arg.starts_with("--socket=") => {
                let value = take_value(arg, "--socket", &mut rest)?;
                socket = Some(PathBuf::from(value));
            }
            _ if arg == "--timeout" || arg.starts_with("--timeout=") => {
                let value = take_value(arg, "--timeout", &mut rest)?;
                timeout_secs = Some(parse_timeout(&value)?);
            }
            _ if arg.starts_with('-') => {
                return Err(UsageError::new(format!("unknown option '{arg}'")));
            }
            _ => positionals.push(arg),
        }
    }

    let timeout_secs = timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    let command = match subcommand.as_str() {
        "help" | "--help" | "-h" => Command::Help,
        "list" => {
            no_positionals("list", &positionals)?;
            Command::List
        }
        "version" => {
            no_positionals("version", &positionals)?;
            Command::Version
        }
        "status" => Command::Status {
            target: one_target("status", &positionals)?,
        },
        "connect" => Command::Connect {
            target: one_target("connect", &positionals)?,
            timeout_secs,
            wait,
        },
        "disconnect" => Command::Disconnect {
            target: one_target("disconnect", &positionals)?,
        },
        "connect-all" => {
            no_positionals("connect-all", &positionals)?;
            Command::ConnectAll { timeout_secs, wait }
        }
        "disconnect-all" => {
            no_positionals("disconnect-all", &positionals)?;
            Command::DisconnectAll
        }
        other => {
            return Err(UsageError::new(format!("unknown command '{other}'")));
        }
    };

    Ok(Invocation {
        command,
        json,
        socket,
    })
}

/// Read a flag value written either as `--flag value` or `--flag=value`.
fn take_value<'a, I>(arg: &'a str, flag: &str, rest: &mut I) -> Result<String, UsageError>
where
    I: Iterator<Item = &'a str>,
{
    if let Some(inline) = arg.strip_prefix(&format!("{flag}=")) {
        if inline.is_empty() {
            return Err(UsageError::new(format!("{flag} requires a value")));
        }
        return Ok(inline.to_string());
    }
    match rest.next() {
        Some(value) if !value.starts_with('-') => Ok(value.to_string()),
        _ => Err(UsageError::new(format!("{flag} requires a value"))),
    }
}

fn parse_timeout(raw: &str) -> Result<u64, UsageError> {
    let secs: u64 = raw
        .parse()
        .map_err(|_| UsageError::new(format!("--timeout expects whole seconds, got '{raw}'")))?;
    if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&secs) {
        return Err(UsageError::new(format!(
            "--timeout must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS} seconds, got {secs}"
        )));
    }
    Ok(secs)
}

fn one_target(command: &str, positionals: &[&str]) -> Result<String, UsageError> {
    match positionals {
        [] => Err(UsageError::new(format!(
            "'{command}' needs a target (forward id or name)"
        ))),
        [target] => Ok((*target).to_string()),
        _ => Err(UsageError::new(format!(
            "'{command}' takes exactly one target, got {}",
            positionals.len()
        ))),
    }
}

fn no_positionals(command: &str, positionals: &[&str]) -> Result<(), UsageError> {
    if positionals.is_empty() {
        Ok(())
    } else {
        Err(UsageError::new(format!(
            "'{command}' takes no arguments, got '{}'",
            positionals[0]
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(args: &[&str]) -> Result<Invocation, UsageError> {
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        parse(&owned)
    }

    #[test]
    fn every_subcommand_is_recognized_by_the_dispatch_probe() {
        for word in [
            "list",
            "status",
            "connect",
            "disconnect",
            "connect-all",
            "disconnect-all",
            "version",
            "help",
            "--help",
            "-h",
        ] {
            assert!(is_cli_subcommand(word), "{word} must start a CLI run");
        }
    }

    /// The GUI's own flags must fall through to `run()` untouched — a regression
    /// here would break launch-at-login.
    #[test]
    fn gui_flags_are_not_cli_subcommands() {
        for word in ["--minimized", "--foo", "", "Tunnel Pilot"] {
            assert!(!is_cli_subcommand(word), "{word} must launch the GUI");
        }
    }

    #[test]
    fn list_parses_with_and_without_json() {
        let inv = parse_str(&["list"]).expect("parse list");
        assert_eq!(inv.command, Command::List);
        assert!(!inv.json);
        assert!(inv.socket.is_none());

        let inv = parse_str(&["list", "--json"]).expect("parse list --json");
        assert!(inv.json);
    }

    #[test]
    fn status_requires_exactly_one_target() {
        let inv = parse_str(&["status", "prod-db"]).expect("parse status");
        assert_eq!(
            inv.command,
            Command::Status {
                target: "prod-db".into()
            }
        );
        assert!(
            parse_str(&["status"]).is_err(),
            "missing target is a usage error"
        );
        assert!(
            parse_str(&["status", "a", "b"]).is_err(),
            "two targets is a usage error"
        );
    }

    #[test]
    fn connect_defaults_to_waiting_with_the_default_timeout() {
        let inv = parse_str(&["connect", "prod-db"]).expect("parse connect");
        assert_eq!(
            inv.command,
            Command::Connect {
                target: "prod-db".into(),
                timeout_secs: DEFAULT_TIMEOUT_SECS,
                wait: true,
            }
        );
    }

    #[test]
    fn connect_honors_no_wait_and_timeout_in_both_forms() {
        let inv = parse_str(&["connect", "prod-db", "--no-wait"]).expect("parse");
        assert_eq!(
            inv.command,
            Command::Connect {
                target: "prod-db".into(),
                timeout_secs: DEFAULT_TIMEOUT_SECS,
                wait: false,
            }
        );

        for args in [
            vec!["connect", "prod-db", "--timeout", "120"],
            vec!["connect", "--timeout=120", "prod-db"],
        ] {
            let inv = parse_str(&args).expect("parse timeout form");
            assert_eq!(
                inv.command,
                Command::Connect {
                    target: "prod-db".into(),
                    timeout_secs: 120,
                    wait: true,
                }
            );
        }
    }

    #[test]
    fn timeout_bounds_are_enforced_client_side() {
        for bad in ["0", "301", "-5", "abc", ""] {
            let args = vec![
                "connect".to_string(),
                "x".to_string(),
                format!("--timeout={bad}"),
            ];
            assert!(parse(&args).is_err(), "--timeout={bad} must be rejected");
        }
        assert!(parse_str(&["connect", "x", "--timeout", "1"]).is_ok());
        assert!(parse_str(&["connect", "x", "--timeout", "300"]).is_ok());
        // A missing value must not swallow the next flag.
        assert!(parse_str(&["connect", "x", "--timeout", "--json"]).is_err());
    }

    #[test]
    fn socket_override_parses_in_both_forms() {
        for args in [
            vec!["list", "--socket", "/tmp/tp.sock"],
            vec!["list", "--socket=/tmp/tp.sock"],
        ] {
            let inv = parse_str(&args).expect("parse socket form");
            assert_eq!(inv.socket, Some(PathBuf::from("/tmp/tp.sock")));
        }
        assert!(parse_str(&["list", "--socket"]).is_err());
    }

    #[test]
    fn bulk_and_version_take_no_positionals() {
        assert_eq!(
            parse_str(&["connect-all"]).expect("parse").command,
            Command::ConnectAll {
                timeout_secs: DEFAULT_TIMEOUT_SECS,
                wait: true
            }
        );
        assert_eq!(
            parse_str(&["connect-all", "--no-wait"])
                .expect("parse")
                .command,
            Command::ConnectAll {
                timeout_secs: DEFAULT_TIMEOUT_SECS,
                wait: false
            }
        );
        assert_eq!(
            parse_str(&["disconnect-all"]).expect("parse").command,
            Command::DisconnectAll
        );
        assert_eq!(
            parse_str(&["version"]).expect("parse").command,
            Command::Version
        );
        assert!(parse_str(&["list", "extra"]).is_err());
        assert!(parse_str(&["version", "extra"]).is_err());
    }

    #[test]
    fn help_forms_all_resolve_to_help() {
        for args in [
            vec!["help"],
            vec!["--help"],
            vec!["-h"],
            vec!["list", "--help"],
        ] {
            assert_eq!(parse_str(&args).expect("parse help").command, Command::Help);
        }
    }

    #[test]
    fn unknown_command_and_option_are_usage_errors() {
        let err = parse_str(&["frobnicate"]).expect_err("unknown command");
        assert!(err.message.contains("frobnicate"));
        let err = parse_str(&["list", "--colour"]).expect_err("unknown option");
        assert!(err.message.contains("--colour"));
        assert!(parse(&[]).is_err(), "no command at all is a usage error");
    }

    #[test]
    fn help_text_documents_every_subcommand_and_exit_code() {
        for word in [
            "list",
            "status",
            "connect",
            "disconnect",
            "connect-all",
            "disconnect-all",
            "version",
            "--json",
            "--socket",
            "--timeout",
            "--no-wait",
            "TUNNEL_PILOT_SOCKET",
        ] {
            assert!(HELP_TEXT.contains(word), "help must mention {word}");
        }
    }
}
