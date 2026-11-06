//! Hand-rolled argument parsing for the CLI (spec 03 §20).
//!
//! Deliberately no `clap`: the surface is a fixed set of subcommands with a
//! handful of flags, and neither the GUI nor the lean CLI-only binary should
//! carry an argument-parsing framework for it. Revisit if the surface grows.
//!
//! [`parse`] is pure — it takes the argv tail and returns either an
//! [`Invocation`] or a [`UsageError`], so every branch is unit-testable without
//! a process or a socket.

use std::path::PathBuf;

use crate::cli::protocol::{ForwardFields, MAX_TIMEOUT_MS, MIN_TIMEOUT_MS};
use crate::cli::shim::ShimTarget;

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
        /// `--force`: tear down an already-live tunnel and re-dial it. Without
        /// it, `connect` on a `connected`/`connecting` tunnel is a no-op that
        /// reports the current state (never bounces live TCP sessions).
        force: bool,
    },
    Disconnect {
        target: String,
    },
    ConnectAll {
        timeout_secs: u64,
        wait: bool,
    },
    DisconnectAll,
    /// Symlink this binary onto `$PATH`. Runs entirely locally — no socket, so
    /// it works whether or not the app is running.
    InstallCli {
        /// `--user` / `--system`; `None` lets the shim layer pick the first
        /// writable directory.
        target: Option<ShimTarget>,
    },
    /// Remove the PATH shim. Local, like `install-cli`.
    UninstallCli,
    Version,
    /// `--version` / `-V`: this binary's own version and edition. Local — it
    /// never touches the socket (install scripts call it).
    LocalVersion,
    /// Create a forward. `fields.name` comes from the positional name.
    Add {
        fields: ForwardFields,
        password: PasswordSource,
    },
    /// Change a forward's fields (and optionally switch it to a password).
    Edit {
        target: String,
        fields: ForwardFields,
        password: PasswordSource,
    },
    /// Delete a forward (disconnects it and drops its stored password).
    Remove {
        target: String,
    },
    /// Store a password for a forward (prompted, or read from stdin).
    SetPassword {
        target: String,
        source: PasswordSource,
    },
    /// Delete a forward's stored password.
    ClearPassword {
        target: String,
    },
    /// Run the headless engine + control socket in the foreground.
    Daemon(DaemonOptions),
    /// Manage the background service that runs `daemon` at login/boot.
    Service(ServiceAction),
    /// Web UI administration (`tunnel-pilot-cli` only).
    Web(WebAction),
}

/// `web` subcommands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAction {
    /// Set (or change) the admin password; `stdin` reads it from stdin.
    SetPassword { stdin: bool },
    /// Report whether an admin password is configured.
    Status,
}

/// Where a password comes from. Never a flag value: argv is visible in `ps`
/// and shell history, so a secret on the command line is not offered at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PasswordSource {
    /// No password for this invocation.
    #[default]
    None,
    /// Prompt on the controlling terminal with echo off (`--password`).
    Prompt,
    /// Read one line from stdin (`--password-stdin`), for scripts.
    Stdin,
}

/// `daemon` flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DaemonOptions {
    /// Connect every forward once the daemon is up (`--connect-all`).
    pub connect_all: bool,
    /// Connect these forwards (id or name) once up (`--connect`, repeatable).
    pub connect: Vec<String>,
    /// Serve the web UI (`--web`).
    pub web: bool,
    /// Web UI listen address (`--bind <addr[:port]>`); `None` = 127.0.0.1:1855.
    pub bind: Option<String>,
    /// Behind a reverse proxy: take the client IP / scheme / host from the
    /// `X-Forwarded-*` headers (`--trust-proxy`). Off by default — without a
    /// proxy those headers are attacker-controlled.
    pub trust_proxy: bool,
    /// Extra host names the web UI answers to (`--public-host`, repeatable),
    /// e.g. the proxy's domain. IP literals and `localhost` are always allowed;
    /// any other `Host` is refused (DNS-rebinding defence).
    pub public_hosts: Vec<String>,
    /// `service install` only: allow a root (system) service to serve the web
    /// UI (`--allow-root`). Refused by default.
    pub allow_root: bool,
}

/// `service` subcommands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServiceAction {
    /// Write + enable the systemd user unit / launchd agent and start it.
    Install(DaemonOptions),
    /// Stop + disable + remove it.
    Uninstall,
    /// Report whether it is installed / running.
    Status,
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
            | "install-cli"
            | "uninstall-cli"
            | "version"
            | "--version"
            | "-V"
            | "add"
            | "edit"
            | "remove"
            | "rm"
            | "set-password"
            | "clear-password"
            | "daemon"
            | "service"
            | "web"
            | "help"
            | "--help"
            | "-h"
    )
}

/// The program name of this product (`tunnel-pilot` or `tunnel-pilot-cli`).
pub const PROGRAM: &str = crate::cli::PROGRAM;

const HELP_TUNNELS: &str = "\
TUNNELS:
    list                       List every configured forward and its status
    status <id|name>           Show one forward's status, endpoints and stats
    connect <id|name>          Connect a forward and wait for a terminal status
                               (already connected/connecting: reports it, no re-dial)
    disconnect <id|name>       Disconnect a forward (waits for teardown)
    connect-all                Connect every forward ('Start All')
    disconnect-all             Disconnect every live forward ('Stop All')

FORWARDS:
    add <name> --ssh <user@host[:port]> --forward <[bind:]lport:host:rport>
               (--identity <keyfile> | --password | --password-stdin)
                               Create a forward
    edit <id|name> [field options]
                               Change a forward (disconnects it if live)
    remove <id|name>           Delete a forward and its stored password (alias: rm)
    set-password <id|name> [--password-stdin]
                               Store a password (prompted with echo off)
    clear-password <id|name>   Delete the stored password
";

const HELP_SERVER: &str = "
SERVER:
    daemon [--connect-all] [--connect <id|name>]... [web options]
                               Run the tunnel engine in the foreground (Ctrl-C /
                               SIGTERM disconnects and exits)
    service install [daemon options] [--allow-root]
                               Run the daemon in the background at login/boot
                               (systemd unit on Linux, launchd agent on macOS).
                               As root, --web is refused unless --allow-root
    service uninstall          Stop and remove that service
    service status             Show whether the service is installed and running
    web set-password [--password-stdin]
                               Set the web UI admin password (signs everyone out)
    web status                 Show whether an admin password is set

WEB OPTIONS (daemon / service install):
    --web                      Serve the web UI (login required; set a password first)
    --bind <addr[:port]>       Listen address (default 127.0.0.1:1855; 0.0.0.0 = all)
    --public-host <name>       A host name the UI is reached by (repeatable), e.g. the
                               HTTPS proxy's domain. IPs and localhost always work;
                               other names are refused (DNS-rebinding defence)
    --trust-proxy              Behind a reverse proxy: trust X-Forwarded-For/-Proto/-Host
                               for the client IP, HTTPS and host checks
";

const HELP_DESKTOP: &str = "
DESKTOP:
    install-cli                Symlink this binary onto PATH as 'tunnel-pilot'
    uninstall-cli              Remove that symlink
";

const HELP_COMMON_TAIL: &str = "
OTHER:
    version                    CLI version + version of the running app/daemon
    --version, -V              This binary's version and edition (no app needed)
    help                       Show this help

OPTIONS:
    --json                     Print the raw JSON response payload
    --socket <path>            Control socket path (default: app config dir)
    --timeout <secs>           Wait budget for connect/connect-all (1-300, default 30)
    --no-wait                  Return as soon as the connect is dispatched
    --force                    connect only: bounce an already-connected tunnel

FIELD OPTIONS (add / edit):
    --name <name>              edit only: rename (add takes the name positionally)
    --ssh <user@host[:port]>   SSH login; edit also accepts a bare host
    --ssh-port <port>          SSH port (default 22)
    --forward, -L <spec>       [bind:]localPort:remoteHost:remotePort, as in 'ssh -L'
                               (bind defaults to 127.0.0.1)
    --identity <keyfile>       Private key file (switches edit to key auth and
                               clears a stored password)
    --password                 Prompt for a password (echo off)
    --password-stdin           Read the password from the first line of stdin
    --keepalive <secs>         Keep-alive interval (0 = default)
    --keepalive-max <n>        Missed keep-alives before reconnect (0 = default)
    --tag <tag>                Add a tag (repeatable; on edit, replaces the tags)
    --no-tags                  edit only: remove every tag
    A password is never accepted as a flag value (it would land in ps/history).

TARGETS:
    A target is a forward id (exact) or name (case-insensitive, exact).
    Ambiguous names are rejected — use the id.

EXIT CODES:
    0 ok   1 internal/IO   2 usage   3 app/daemon not running
    4 target not found or ambiguous   5 operation ended in error   6 timed out
    A protocol-version mismatch (stale binary vs running app) also exits 4, with
    an 'unsupported protocol version' message. add / edit validation failures
    exit 2.

ENVIRONMENT:
    TUNNEL_PILOT_SOCKET        Socket path override (both owner and CLI)
    TUNNEL_PILOT_CONFIG_DIR    Config dir override (default: this product's own
                               config dir); also where CLI-stored passwords go
";

/// The full usage text for THIS product: the shared sections plus either the
/// desktop (PATH shim) or the server (daemon / service / web) commands, with the
/// program name filled in.
pub fn help_text() -> String {
    let edition_section = if cfg!(feature = "gui") {
        HELP_DESKTOP
    } else {
        HELP_SERVER
    };
    let intro = if cfg!(feature = "gui") {
        "SSH port forwards from a terminal (talks to the running desktop app)"
    } else {
        "SSH port forwards on a server (daemon, CLI and web UI)"
    };
    format!(
        "{PROGRAM} — {intro}\n\nUSAGE:\n    {PROGRAM} <command> [options]\n\n\
         {HELP_TUNNELS}{edition_section}{HELP_COMMON_TAIL}"
    )
}

/// Parse the argv tail (everything after the program name).
pub fn parse(args: &[String]) -> Result<Invocation, UsageError> {
    let Some(subcommand) = args.first() else {
        return Err(UsageError::new("missing command"));
    };

    let mut json = false;
    let mut socket: Option<PathBuf> = None;
    let mut timeout_secs: Option<u64> = None;
    let mut wait = true;
    let mut force = false;
    let mut user = false;
    let mut system = false;
    let mut positionals: Vec<&str> = Vec::new();
    // add / edit field flags.
    let mut fields = ForwardFields::default();
    let mut field_flag: Option<&str> = None;
    let mut password = PasswordSource::None;
    let mut no_identity_conflict = false;
    let mut tags: Option<Vec<String>> = None;
    // daemon / service install flags.
    let mut daemon = DaemonOptions::default();
    let mut daemon_flag: Option<&str> = None;

    let mut rest = args[1..].iter().map(|s| s.as_str()).peekable();
    while let Some(arg) = rest.next() {
        match arg {
            "--json" => json = true,
            "--no-wait" => wait = false,
            "--force" => force = true,
            "--user" => user = true,
            "--system" => system = true,
            "--password" => {
                set_password_source(&mut password, PasswordSource::Prompt)?;
            }
            "--password-stdin" => {
                set_password_source(&mut password, PasswordSource::Stdin)?;
            }
            "--no-tags" => {
                field_flag = Some("--no-tags");
                tags = Some(Vec::new());
            }
            "--connect-all" => {
                daemon_flag = Some("--connect-all");
                daemon.connect_all = true;
            }
            "--web" => {
                daemon_flag = Some("--web");
                daemon.web = true;
            }
            _ if is_flag(arg, "--bind") => {
                daemon_flag = Some("--bind");
                daemon.bind = Some(take_value(arg, "--bind", &mut rest)?);
            }
            "--trust-proxy" => {
                daemon_flag = Some("--trust-proxy");
                daemon.trust_proxy = true;
            }
            _ if is_flag(arg, "--public-host") => {
                daemon_flag = Some("--public-host");
                let host = non_blank(
                    "--public-host",
                    take_value(arg, "--public-host", &mut rest)?,
                )?;
                daemon.public_hosts.push(host.to_ascii_lowercase());
            }
            "--allow-root" => {
                daemon_flag = Some("--allow-root");
                daemon.allow_root = true;
            }
            _ if is_flag(arg, "--connect") => {
                daemon_flag = Some("--connect");
                daemon
                    .connect
                    .push(take_value(arg, "--connect", &mut rest)?);
            }
            _ if is_flag(arg, "--name") => {
                field_flag = Some("--name");
                fields.name = Some(non_blank("--name", take_value(arg, "--name", &mut rest)?)?);
            }
            _ if is_flag(arg, "--ssh") => {
                field_flag = Some("--ssh");
                let (login_user, host, port) = parse_ssh(&take_value(arg, "--ssh", &mut rest)?)?;
                if login_user.is_some() {
                    fields.ssh_username = login_user;
                }
                fields.ssh_host = Some(host);
                if port.is_some() && fields.ssh_port.is_none() {
                    fields.ssh_port = port;
                }
            }
            _ if is_flag(arg, "--ssh-port") => {
                field_flag = Some("--ssh-port");
                fields.ssh_port = Some(parse_port(
                    "--ssh-port",
                    &take_value(arg, "--ssh-port", &mut rest)?,
                )?);
            }
            _ if is_flag(arg, "--forward") || is_flag(arg, "-L") => {
                field_flag = Some("--forward");
                let flag = if arg.starts_with("-L") {
                    "-L"
                } else {
                    "--forward"
                };
                let spec = parse_forward(&take_value(arg, flag, &mut rest)?)?;
                fields.local_bind_address = Some(spec.bind);
                fields.local_port = Some(spec.local_port);
                fields.remote_host = Some(spec.remote_host);
                fields.remote_port = Some(spec.remote_port);
            }
            _ if is_flag(arg, "--identity") => {
                field_flag = Some("--identity");
                no_identity_conflict = true;
                fields.identity_file_path = Some(non_blank(
                    "--identity",
                    take_value(arg, "--identity", &mut rest)?,
                )?);
            }
            _ if is_flag(arg, "--keepalive") => {
                field_flag = Some("--keepalive");
                fields.keep_alive_interval_sec = Some(parse_u32(
                    "--keepalive",
                    &take_value(arg, "--keepalive", &mut rest)?,
                )?);
            }
            _ if is_flag(arg, "--keepalive-max") => {
                field_flag = Some("--keepalive-max");
                fields.keep_alive_max_count = Some(parse_u32(
                    "--keepalive-max",
                    &take_value(arg, "--keepalive-max", &mut rest)?,
                )?);
            }
            _ if is_flag(arg, "--tag") => {
                field_flag = Some("--tag");
                let tag = non_blank("--tag", take_value(arg, "--tag", &mut rest)?)?;
                tags.get_or_insert_with(Vec::new).push(tag);
            }
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
    if force && subcommand != "connect" {
        return Err(UsageError::new(format!(
            "--force only applies to 'connect', not '{subcommand}'"
        )));
    }
    if (user || system) && subcommand != "install-cli" {
        return Err(UsageError::new(format!(
            "--user/--system only apply to 'install-cli', not '{subcommand}'"
        )));
    }
    if user && system {
        return Err(UsageError::new(
            "--user and --system are mutually exclusive",
        ));
    }
    let is_field_command = matches!(subcommand.as_str(), "add" | "edit");
    if let Some(flag) = field_flag {
        if !is_field_command {
            return Err(UsageError::new(format!(
                "{flag} only applies to 'add' / 'edit', not '{subcommand}'"
            )));
        }
    }
    if password != PasswordSource::None
        && !matches!(subcommand.as_str(), "add" | "edit" | "set-password" | "web")
    {
        return Err(UsageError::new(format!(
            "--password/--password-stdin only apply to 'add', 'edit', 'set-password' and \
             'web set-password', not '{subcommand}'"
        )));
    }
    if no_identity_conflict && password != PasswordSource::None {
        return Err(UsageError::new(
            "use either --identity or a password, not both",
        ));
    }
    if !daemon.web
        && (daemon.bind.is_some() || daemon.trust_proxy || !daemon.public_hosts.is_empty())
    {
        return Err(UsageError::new(
            "--bind, --trust-proxy and --public-host need --web",
        ));
    }
    if daemon.allow_root && subcommand != "service" {
        return Err(UsageError::new(
            "--allow-root only applies to 'service install'",
        ));
    }
    if let Some(flag) = daemon_flag {
        let daemon_like = subcommand == "daemon"
            || (subcommand == "service" && positionals.first() == Some(&"install"));
        if !daemon_like {
            return Err(UsageError::new(format!(
                "{flag} only applies to 'daemon' / 'service install', not '{subcommand}'"
            )));
        }
    }
    fields.tags = tags;
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
            force,
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
        "install-cli" => {
            no_positionals("install-cli", &positionals)?;
            let target = match (user, system) {
                (true, _) => Some(ShimTarget::UserLocal),
                (_, true) => Some(ShimTarget::UsrLocal),
                _ => None,
            };
            Command::InstallCli { target }
        }
        "uninstall-cli" => {
            no_positionals("uninstall-cli", &positionals)?;
            Command::UninstallCli
        }
        "--version" | "-V" => {
            no_positionals(subcommand, &positionals)?;
            Command::LocalVersion
        }
        "add" => {
            let name = one_target("add", &positionals)?;
            if fields.name.is_some() {
                return Err(UsageError::new(
                    "'add' takes the name positionally; --name is for 'edit'",
                ));
            }
            if fields.ssh_host.is_none() || fields.ssh_username.is_none() {
                return Err(UsageError::new(
                    "'add' needs --ssh <user@host[:port]> (with a user)",
                ));
            }
            if fields.local_port.is_none() {
                return Err(UsageError::new(
                    "'add' needs --forward <[bind:]localPort:remoteHost:remotePort>",
                ));
            }
            if fields.identity_file_path.is_none() && password == PasswordSource::None {
                return Err(UsageError::new(
                    "'add' needs an auth method: --identity <keyfile>, --password or \
                     --password-stdin",
                ));
            }
            fields.name = Some(non_blank("name", name)?);
            Command::Add { fields, password }
        }
        "edit" => {
            let target = one_target("edit", &positionals)?;
            if fields.is_empty() && password == PasswordSource::None {
                return Err(UsageError::new(
                    "'edit' needs at least one field option (see 'tunnel-pilot help')",
                ));
            }
            Command::Edit {
                target,
                fields,
                password,
            }
        }
        "remove" | "rm" => Command::Remove {
            target: one_target("remove", &positionals)?,
        },
        "set-password" => {
            let target = one_target("set-password", &positionals)?;
            let source = match password {
                PasswordSource::None => PasswordSource::Prompt,
                other => other,
            };
            Command::SetPassword { target, source }
        }
        "clear-password" => Command::ClearPassword {
            target: one_target("clear-password", &positionals)?,
        },
        "daemon" => {
            no_positionals("daemon", &positionals)?;
            Command::Daemon(daemon)
        }
        "service" => match positionals.as_slice() {
            ["install"] => Command::Service(ServiceAction::Install(daemon)),
            ["uninstall"] => Command::Service(ServiceAction::Uninstall),
            ["status"] => Command::Service(ServiceAction::Status),
            [] => {
                return Err(UsageError::new(
                    "'service' needs an action: install, uninstall or status",
                ))
            }
            [other, ..] => {
                return Err(UsageError::new(format!(
                    "unknown service action '{other}' (install, uninstall, status)"
                )))
            }
        },
        "web" => match positionals.as_slice() {
            ["set-password"] => Command::Web(WebAction::SetPassword {
                stdin: password == PasswordSource::Stdin,
            }),
            ["status"] if password == PasswordSource::None => Command::Web(WebAction::Status),
            _ => {
                return Err(UsageError::new(
                    "usage: web set-password [--password-stdin] | web status",
                ))
            }
        },
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

/// Whether `arg` is `flag` itself or its `flag=value` form.
fn is_flag(arg: &str, flag: &str) -> bool {
    arg == flag
        || arg
            .strip_prefix(flag)
            .is_some_and(|rest| rest.starts_with('='))
}

fn set_password_source(
    current: &mut PasswordSource,
    next: PasswordSource,
) -> Result<(), UsageError> {
    if *current != PasswordSource::None && *current != next {
        return Err(UsageError::new(
            "--password and --password-stdin are mutually exclusive",
        ));
    }
    *current = next;
    Ok(())
}

fn non_blank(what: &str, value: String) -> Result<String, UsageError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(UsageError::new(format!("{what} must not be empty")));
    }
    Ok(trimmed.to_string())
}

fn parse_port(what: &str, raw: &str) -> Result<u16, UsageError> {
    match raw.trim().parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err(UsageError::new(format!(
            "{what} expects a port between 1 and 65535, got '{raw}'"
        ))),
    }
}

fn parse_u32(what: &str, raw: &str) -> Result<u32, UsageError> {
    raw.trim()
        .parse::<u32>()
        .map_err(|_| UsageError::new(format!("{what} expects a whole number, got '{raw}'")))
}

/// Strip one pair of IPv6 brackets: `[::1]` → `::1`.
fn unbracket(host: &str) -> &str {
    host.strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host)
}

/// `--ssh` value: `[user@]host[:port]`, host may be a bracketed IPv6 literal.
/// A bare (unbracketed) IPv6 address is taken whole as the host. Pure.
pub fn parse_ssh(raw: &str) -> Result<(Option<String>, String, Option<u16>), UsageError> {
    let raw = raw.trim();
    let (user, host_port) = match raw.rsplit_once('@') {
        Some((user, rest)) => {
            if user.is_empty() {
                return Err(UsageError::new(format!("--ssh has an empty user: '{raw}'")));
            }
            (Some(user.to_string()), rest)
        }
        None => (None, raw),
    };
    let (host, port) = if let Some(after_open) = host_port.strip_prefix('[') {
        let Some((inner, tail)) = after_open.split_once(']') else {
            return Err(UsageError::new(format!(
                "--ssh has an unclosed '[': '{raw}'"
            )));
        };
        let port = match tail.strip_prefix(':') {
            Some(p) => Some(parse_port("--ssh port", p)?),
            None if tail.is_empty() => None,
            None => {
                return Err(UsageError::new(format!(
                    "--ssh: unexpected '{tail}' in '{raw}'"
                )))
            }
        };
        (inner.to_string(), port)
    } else if host_port.matches(':').count() == 1 {
        let (h, p) = host_port.split_once(':').unwrap_or((host_port, ""));
        (h.to_string(), Some(parse_port("--ssh port", p)?))
    } else {
        (host_port.to_string(), None)
    };
    if host.is_empty() {
        return Err(UsageError::new(format!("--ssh has an empty host: '{raw}'")));
    }
    Ok((user, host, port))
}

/// A parsed `--forward` / `-L` spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardSpec {
    pub bind: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
}

/// `--forward` value in `ssh -L` syntax: `[bind:]localPort:remoteHost:remotePort`.
/// `bind` and `remoteHost` may be bracketed IPv6 literals; `*` binds all
/// interfaces (`0.0.0.0`), as in OpenSSH. Pure.
pub fn parse_forward(raw: &str) -> Result<ForwardSpec, UsageError> {
    let bad = || {
        UsageError::new(format!(
            "--forward expects [bind:]localPort:remoteHost:remotePort, got '{raw}'"
        ))
    };
    let raw = raw.trim();
    // remotePort: after the last ':'.
    let (rest, remote_port) = raw.rsplit_once(':').ok_or_else(bad)?;
    let remote_port = parse_port("--forward remote port", remote_port)?;
    // remoteHost: a bracketed literal, or the segment after the next ':'.
    let (rest, remote_host) = if rest.ends_with(']') {
        let open = rest.rfind('[').ok_or_else(bad)?;
        let head = rest[..open].strip_suffix(':').ok_or_else(bad)?;
        (head, unbracket(&rest[open..]).to_string())
    } else {
        let (head, host) = rest.rsplit_once(':').ok_or_else(bad)?;
        (head, host.to_string())
    };
    if remote_host.is_empty() {
        return Err(bad());
    }
    // localPort, with an optional bind address before it.
    let (bind, local_port) = match rest.rsplit_once(':') {
        Some((bind, port)) => (unbracket(bind).to_string(), port),
        None => ("127.0.0.1".to_string(), rest),
    };
    let local_port = parse_port("--forward local port", local_port)?;
    let bind = match bind.as_str() {
        "" => return Err(bad()),
        "*" => "0.0.0.0".to_string(),
        _ => bind,
    };
    Ok(ForwardSpec {
        bind,
        local_port,
        remote_host,
        remote_port,
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
            "install-cli",
            "uninstall-cli",
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
                force: false,
            }
        );
    }

    /// `--force` is the opt-in that restores the bounce-and-redial behaviour;
    /// without it `connect` is idempotent (service-side guard).
    #[test]
    fn connect_force_is_opt_in_and_connect_only() {
        let inv = parse_str(&["connect", "prod-db", "--force"]).expect("parse --force");
        assert_eq!(
            inv.command,
            Command::Connect {
                target: "prod-db".into(),
                timeout_secs: DEFAULT_TIMEOUT_SECS,
                wait: true,
                force: true,
            }
        );

        for args in [
            vec!["connect-all", "--force"],
            vec!["disconnect", "prod-db", "--force"],
            vec!["list", "--force"],
        ] {
            let err = parse_str(&args).expect_err("--force is connect-only");
            assert!(err.message.contains("--force"), "{}", err.message);
        }
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
                force: false,
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
                    force: false,
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

    /// The shim subcommands are local: they must parse without a target and
    /// pick their directory through the two exclusive flags.
    #[test]
    fn shim_subcommands_parse_with_an_optional_target() {
        assert_eq!(
            parse_str(&["install-cli"]).expect("parse").command,
            Command::InstallCli { target: None }
        );
        assert_eq!(
            parse_str(&["install-cli", "--user"])
                .expect("parse")
                .command,
            Command::InstallCli {
                target: Some(ShimTarget::UserLocal)
            }
        );
        assert_eq!(
            parse_str(&["install-cli", "--system"])
                .expect("parse")
                .command,
            Command::InstallCli {
                target: Some(ShimTarget::UsrLocal)
            }
        );
        assert_eq!(
            parse_str(&["uninstall-cli"]).expect("parse").command,
            Command::UninstallCli
        );
        assert!(parse_str(&["install-cli", "--json"]).expect("parse").json);
    }

    #[test]
    fn shim_flags_are_exclusive_and_install_cli_only() {
        let err =
            parse_str(&["install-cli", "--user", "--system"]).expect_err("mutually exclusive");
        assert!(
            err.message.contains("mutually exclusive"),
            "{}",
            err.message
        );
        for args in [
            vec!["list", "--user"],
            vec!["uninstall-cli", "--system"],
            vec!["connect", "x", "--user"],
        ] {
            let err = parse_str(&args).expect_err("--user/--system are install-cli only");
            assert!(err.message.contains("install-cli"), "{}", err.message);
        }
        assert!(
            parse_str(&["install-cli", "extra"]).is_err(),
            "install-cli takes no positionals"
        );
        assert!(
            parse_str(&["uninstall-cli", "extra"]).is_err(),
            "uninstall-cli takes no positionals"
        );
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
            "add",
            "set-password",
            "version",
            "--json",
            "--socket",
            "--timeout",
            "--no-wait",
            "--force",
            "TUNNEL_PILOT_SOCKET",
        ] {
            assert!(help_text().contains(word), "help must mention {word}");
        }
        // Each product documents only its own commands (spec 03 §22).
        let desktop_only = ["install-cli", "uninstall-cli"];
        let server_only = [
            "\n    daemon ",
            "service install",
            "web set-password",
            "1855",
        ];
        let (present, absent) = if cfg!(feature = "gui") {
            (desktop_only.as_slice(), server_only.as_slice())
        } else {
            (server_only.as_slice(), desktop_only.as_slice())
        };
        for word in present {
            assert!(help_text().contains(word), "help must mention {word}");
        }
        for word in absent {
            assert!(!help_text().contains(word), "help must not mention {word}");
        }
        assert!(help_text().starts_with(PROGRAM));
    }
    // --- spec 03 §20/§21: CRUD, daemon, service, desktop ---

    #[test]
    fn new_subcommands_are_recognized_by_the_dispatch_probe() {
        for word in [
            "add",
            "edit",
            "remove",
            "rm",
            "set-password",
            "clear-password",
            "daemon",
            "service",
            "web",
            "--version",
            "-V",
        ] {
            assert!(is_cli_subcommand(word), "{word} must start a CLI run");
        }
    }

    #[test]
    fn parse_ssh_handles_users_ports_and_ipv6() {
        assert_eq!(
            parse_ssh("deploy@bastion").expect("ssh"),
            (Some("deploy".into()), "bastion".into(), None)
        );
        assert_eq!(
            parse_ssh("deploy@bastion:2222").expect("ssh"),
            (Some("deploy".into()), "bastion".into(), Some(2222))
        );
        assert_eq!(
            parse_ssh("ops@[::1]:2200").expect("ssh"),
            (Some("ops".into()), "::1".into(), Some(2200))
        );
        assert_eq!(
            parse_ssh("fe80::1").expect("bare ipv6"),
            (None, "fe80::1".into(), None)
        );
        assert!(parse_ssh("@host").is_err());
        assert!(parse_ssh("user@").is_err());
        assert!(parse_ssh("user@host:0").is_err());
        assert!(parse_ssh("user@[::1").is_err());
    }

    #[test]
    fn parse_forward_follows_ssh_dash_l_syntax() {
        let spec = parse_forward("5432:db.internal:5432").expect("spec");
        assert_eq!(spec.bind, "127.0.0.1");
        assert_eq!(spec.local_port, 5432);
        assert_eq!(spec.remote_host, "db.internal");
        assert_eq!(spec.remote_port, 5432);

        let spec = parse_forward("0.0.0.0:8080:web:80").expect("bind");
        assert_eq!(spec.bind, "0.0.0.0");
        assert_eq!(spec.local_port, 8080);

        let spec = parse_forward("*:8080:[fe80::1]:80").expect("wildcard + ipv6");
        assert_eq!(spec.bind, "0.0.0.0");
        assert_eq!(spec.remote_host, "fe80::1");

        let spec = parse_forward("[::1]:9000:svc:90").expect("ipv6 bind");
        assert_eq!(spec.bind, "::1");

        for bad in [
            "5432",
            "5432:db",
            "x:db:5432",
            "5432:db:0",
            ":5432:db:1",
            "1::2",
        ] {
            assert!(parse_forward(bad).is_err(), "{bad} must be rejected");
        }
    }

    #[test]
    fn add_requires_ssh_forward_and_exactly_one_auth() {
        let ok = parse_str(&[
            "add",
            "db",
            "--ssh",
            "deploy@bastion:2222",
            "-L",
            "15432:db.internal:5432",
            "--identity",
            "~/.ssh/id",
            "--tag",
            "prod",
        ])
        .expect("add");
        let Command::Add { fields, password } = ok.command else {
            panic!("expected add");
        };
        assert_eq!(fields.name.as_deref(), Some("db"));
        assert_eq!(fields.ssh_port, Some(2222));
        assert_eq!(fields.local_port, Some(15432));
        assert_eq!(fields.tags, Some(vec!["prod".to_string()]));
        assert_eq!(password, PasswordSource::None);

        let with_pw = parse_str(&[
            "add",
            "db",
            "--ssh=deploy@bastion",
            "--forward=1:h:2",
            "--password-stdin",
        ])
        .expect("add with password");
        assert!(matches!(
            with_pw.command,
            Command::Add {
                password: PasswordSource::Stdin,
                ..
            }
        ));

        for bad in [
            vec!["add", "db", "--forward", "1:h:2", "--identity", "k"],
            vec![
                "add",
                "db",
                "--ssh",
                "host",
                "--forward",
                "1:h:2",
                "--identity",
                "k",
            ],
            vec!["add", "db", "--ssh", "u@h", "--identity", "k"],
            vec!["add", "db", "--ssh", "u@h", "--forward", "1:h:2"],
            vec![
                "add",
                "db",
                "--ssh",
                "u@h",
                "--forward",
                "1:h:2",
                "--identity",
                "k",
                "--password",
            ],
            vec![
                "add",
                "--ssh",
                "u@h",
                "--forward",
                "1:h:2",
                "--identity",
                "k",
            ],
        ] {
            assert!(parse_str(&bad).is_err(), "{bad:?} must be a usage error");
        }
    }

    #[test]
    fn field_and_daemon_flags_are_scoped_to_their_commands() {
        assert!(parse_str(&["list", "--ssh", "u@h"]).is_err());
        assert!(parse_str(&["connect", "db", "--password"]).is_err());
        assert!(parse_str(&["list", "--connect-all"]).is_err());
        assert!(parse_str(&["service", "status", "--connect-all"]).is_err());
        assert!(parse_str(&["edit", "db"]).is_err(), "edit needs a change");
        assert!(parse_str(&["edit", "db", "--no-tags"]).is_ok());
    }

    #[test]
    fn set_password_defaults_to_a_prompt() {
        let inv = parse_str(&["set-password", "db"]).expect("parse");
        assert_eq!(
            inv.command,
            Command::SetPassword {
                target: "db".into(),
                source: PasswordSource::Prompt
            }
        );
        let inv = parse_str(&["set-password", "db", "--password-stdin"]).expect("parse");
        assert!(matches!(
            inv.command,
            Command::SetPassword {
                source: PasswordSource::Stdin,
                ..
            }
        ));
    }

    #[test]
    fn daemon_service_and_desktop_parse() {
        let inv = parse_str(&[
            "daemon",
            "--connect-all",
            "--connect",
            "db",
            "--connect=web",
        ])
        .expect("daemon");
        assert_eq!(
            inv.command,
            Command::Daemon(DaemonOptions {
                connect_all: true,
                connect: vec!["db".into(), "web".into()],
                ..DaemonOptions::default()
            })
        );
        let inv = parse_str(&["service", "install", "--connect-all"]).expect("service install");
        assert!(matches!(
            inv.command,
            Command::Service(ServiceAction::Install(DaemonOptions {
                connect_all: true,
                ..
            }))
        ));
        assert_eq!(
            parse_str(&["service", "uninstall"])
                .expect("uninstall")
                .command,
            Command::Service(ServiceAction::Uninstall)
        );
        assert!(parse_str(&["service"]).is_err());
        assert!(parse_str(&["service", "restart"]).is_err());
        let inv = parse_str(&["daemon", "--web", "--bind", "0.0.0.0:1855"]).expect("web daemon");
        assert!(matches!(
            inv.command,
            Command::Daemon(DaemonOptions { web: true, bind: Some(ref b), .. }) if b == "0.0.0.0:1855"
        ));
        assert!(
            parse_str(&["daemon", "--bind", "0.0.0.0"]).is_err(),
            "--bind needs --web"
        );
        assert_eq!(
            parse_str(&["web", "set-password", "--password-stdin"])
                .expect("web set-password")
                .command,
            Command::Web(WebAction::SetPassword { stdin: true })
        );
        assert_eq!(
            parse_str(&["web", "status"]).expect("web status").command,
            Command::Web(WebAction::Status)
        );
        assert!(parse_str(&["web"]).is_err());
        assert!(
            parse_str(&["desktop", "install"]).is_err(),
            "desktop install is gone"
        );
        assert_eq!(
            parse_str(&["--version"]).expect("version").command,
            Command::LocalVersion
        );
    }
}
