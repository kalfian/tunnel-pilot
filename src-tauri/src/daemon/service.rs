//! `tunnel-pilot service install|uninstall|status` — keep the daemon running
//! across logins and reboots (spec 03 §21).
//!
//! - **Linux**: a systemd unit. A normal user gets a *user* unit
//!   (`~/.config/systemd/user/tunnel-pilot.service`, `systemctl --user`); root
//!   gets a *system* unit (`/etc/systemd/system/tunnel-pilot.service`), which is
//!   what a server usually wants. A user unit only survives logout when
//!   lingering is enabled — we check and print the one command that fixes it.
//! - **macOS**: a launchd agent (`~/Library/LaunchAgents/<label>.plist`)
//!   loaded into the user's GUI domain.
//!
//! The unit/plist pins the absolute binary path and the resolved config dir
//! (`TUNNEL_PILOT_CONFIG_DIR`), so the service and an interactive shell always
//! agree on the config, the secrets file and the socket — even for a system
//! unit, whose environment has no `$XDG_CONFIG_HOME`.

use std::path::{Path, PathBuf};
use std::process::Command as Process;

use serde::Serialize;

use crate::cli::args::{DaemonOptions, ServiceAction};
use crate::cli::client::{EXIT_INTERNAL, EXIT_OK, EXIT_USAGE};

/// systemd unit name.
pub const UNIT_NAME: &str = "tunnel-pilot.service";
/// launchd label (bundle id + `.daemon`, so it never collides with the app's
/// own login item).
pub const LAUNCHD_LABEL: &str = "com.kalfian.tunnelpilot.daemon";

/// `service status` payload (`--json` prints it verbatim).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    /// `systemd-user`, `systemd-system` or `launchd`.
    pub manager: String,
    pub installed: bool,
    pub running: bool,
    /// The unit / plist file.
    pub path: String,
}

/// Dispatch one `service` action; returns the exit code.
pub fn run(action: &ServiceAction, json: bool) -> i32 {
    let result = match action {
        ServiceAction::Install(options) => install(options),
        ServiceAction::Uninstall => uninstall(),
        ServiceAction::Status => Ok(()),
    };
    if let Err(code) = result {
        return code;
    }
    let status = status();
    if json {
        match serde_json::to_string(&status) {
            Ok(line) => println!("{line}"),
            Err(e) => {
                eprintln!("error: {e}");
                return EXIT_INTERNAL;
            }
        }
    } else {
        println!(
            "service   {}\nmanager   {}\nfile      {}\nrunning   {}",
            if status.installed {
                "installed"
            } else {
                "not installed"
            },
            status.manager,
            status.path,
            if status.running { "yes" } else { "no" }
        );
    }
    EXIT_OK
}

/// Whether a service definition exists for this user (used by `desktop
/// install` to warn about running both owners).
pub fn is_installed() -> bool {
    Platform::detect().is_some_and(|p| p.unit_path().is_file())
}

/// The current service state (best-effort; never fails).
pub fn status() -> ServiceStatus {
    match Platform::detect() {
        Some(platform) => {
            let path = platform.unit_path();
            ServiceStatus {
                manager: platform.manager_name().to_string(),
                installed: path.is_file(),
                running: platform.is_running(),
                path: path.display().to_string(),
            }
        }
        None => ServiceStatus {
            manager: "unsupported".into(),
            installed: false,
            running: false,
            path: String::new(),
        },
    }
}

fn install(options: &DaemonOptions) -> Result<(), i32> {
    let platform = Platform::detect().ok_or_else(unsupported)?;
    let exe = service_binary()?;
    let config_dir = crate::cli::default_config_dir().ok_or_else(|| {
        eprintln!(
            "error: could not resolve the config dir; set {}",
            crate::cli::CONFIG_DIR_ENV
        );
        EXIT_INTERNAL
    })?;
    let mut args = vec!["daemon".to_string()];
    if options.connect_all {
        args.push("--connect-all".into());
    }
    for target in &options.connect {
        args.push("--connect".into());
        args.push(target.clone());
    }
    let mut env = vec![(
        crate::cli::CONFIG_DIR_ENV.to_string(),
        config_dir.display().to_string(),
    )];
    if let Some(socket) = crate::cli::socket_path_from_env() {
        env.push((
            crate::cli::SOCKET_ENV.to_string(),
            socket.display().to_string(),
        ));
    }

    let path = platform.unit_path();
    let contents = match platform {
        Platform::SystemdUser | Platform::SystemdSystem => {
            systemd_unit(&exe, &args, &env, platform == Platform::SystemdSystem)
        }
        Platform::Launchd => launchd_plist(&exe, &args, &env, &launchd_log_path()),
    };
    write_file(&path, &contents)?;
    println!("wrote {}", path.display());
    platform.enable()?;
    platform.post_install_hints();
    Ok(())
}

fn uninstall() -> Result<(), i32> {
    let platform = Platform::detect().ok_or_else(unsupported)?;
    let path = platform.unit_path();
    platform.disable();
    match std::fs::remove_file(&path) {
        Ok(()) => println!("removed {}", path.display()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            println!("no service installed at {}", path.display());
        }
        Err(e) => {
            eprintln!("error: cannot remove {}: {e}", path.display());
            return Err(EXIT_INTERNAL);
        }
    }
    platform.after_remove();
    Ok(())
}

fn unsupported() -> i32 {
    eprintln!("error: no supported service manager (systemd or launchd) on this system");
    EXIT_USAGE
}

/// The binary the service will run: this one, by absolute canonical path. A
/// `target/` dev build is refused (it disappears on `cargo clean`), with the
/// same override the PATH shim honors.
fn service_binary() -> Result<PathBuf, i32> {
    let Some(exe) = crate::cli::shim::current_exe() else {
        eprintln!("error: cannot resolve this binary's path");
        return Err(EXIT_INTERNAL);
    };
    if crate::cli::shim::dev_build_blocks(&exe, crate::cli::shim::dev_shim_allowed()) {
        eprintln!(
            "error: refusing to install a service for a development build ({}); \
             set TUNNEL_PILOT_ALLOW_DEV_SHIM=1 to override",
            exe.display()
        );
        return Err(EXIT_USAGE);
    }
    Ok(exe)
}

fn write_file(path: &Path, contents: &str) -> Result<(), i32> {
    if let Some(dir) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("error: cannot create {}: {e}", dir.display());
            return Err(EXIT_INTERNAL);
        }
    }
    std::fs::write(path, contents).map_err(|e| {
        eprintln!("error: cannot write {}: {e}", path.display());
        EXIT_INTERNAL
    })
}

/// Run a service-manager command, echoing it; `Err` carries a printed reason.
fn run_tool(program: &str, args: &[&str]) -> Result<String, String> {
    let output = Process::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if output.status.success() {
        Ok(stdout)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            stderr.trim()
        ))
    }
}

fn current_uid() -> Option<String> {
    run_tool("id", &["-u"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

fn launchd_log_path() -> PathBuf {
    home_dir()
        .unwrap_or_default()
        .join("Library/Logs/tunnel-pilot-daemon.log")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    SystemdUser,
    SystemdSystem,
    Launchd,
}

impl Platform {
    fn detect() -> Option<Self> {
        if cfg!(target_os = "macos") {
            return Some(Self::Launchd);
        }
        if cfg!(target_os = "linux") {
            let root = current_uid().as_deref() == Some("0");
            return Some(if root {
                Self::SystemdSystem
            } else {
                Self::SystemdUser
            });
        }
        None
    }

    fn manager_name(self) -> &'static str {
        match self {
            Self::SystemdUser => "systemd-user",
            Self::SystemdSystem => "systemd-system",
            Self::Launchd => "launchd",
        }
    }

    fn unit_path(self) -> PathBuf {
        match self {
            Self::SystemdSystem => PathBuf::from("/etc/systemd/system").join(UNIT_NAME),
            Self::SystemdUser => {
                let base = std::env::var_os("XDG_CONFIG_HOME")
                    .filter(|v| !v.is_empty())
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home_dir().unwrap_or_default().join(".config"));
                base.join("systemd/user").join(UNIT_NAME)
            }
            Self::Launchd => home_dir()
                .unwrap_or_default()
                .join("Library/LaunchAgents")
                .join(format!("{LAUNCHD_LABEL}.plist")),
        }
    }

    /// `systemctl` args with `--user` when this is a user unit.
    fn systemctl(self, args: &[&str]) -> Result<String, String> {
        let mut full: Vec<&str> = Vec::new();
        if self == Self::SystemdUser {
            full.push("--user");
        }
        full.extend_from_slice(args);
        run_tool("systemctl", &full)
    }

    fn launchd_target(self) -> Option<String> {
        current_uid().map(|uid| format!("gui/{uid}/{LAUNCHD_LABEL}"))
    }

    fn enable(self) -> Result<(), i32> {
        let result = match self {
            Self::SystemdUser | Self::SystemdSystem => self
                .systemctl(&["daemon-reload"])
                .and_then(|_| self.systemctl(&["enable", "--now", UNIT_NAME]))
                // `enable --now` does not restart an already-running unit, so a
                // re-install (new flags / new binary) needs an explicit restart.
                .and_then(|_| self.systemctl(&["restart", UNIT_NAME])),
            Self::Launchd => {
                let Some(uid) = current_uid() else {
                    eprintln!("error: cannot determine the current uid");
                    return Err(EXIT_INTERNAL);
                };
                // Unload a previous definition first so a re-install applies.
                if let Some(target) = self.launchd_target() {
                    let _ = run_tool("launchctl", &["bootout", &target]);
                }
                let plist = self.unit_path().display().to_string();
                run_tool("launchctl", &["bootstrap", &format!("gui/{uid}"), &plist])
            }
        };
        match result {
            Ok(_) => {
                println!("service enabled and started ({})", self.manager_name());
                Ok(())
            }
            Err(reason) => {
                eprintln!("error: {reason}");
                eprintln!(
                    "The service file is in place; start it manually once the service \
                     manager is available, or run 'tunnel-pilot daemon' in a terminal."
                );
                Err(EXIT_INTERNAL)
            }
        }
    }

    fn disable(self) {
        let result = match self {
            Self::SystemdUser | Self::SystemdSystem => {
                self.systemctl(&["disable", "--now", UNIT_NAME]).map(|_| ())
            }
            Self::Launchd => match self.launchd_target() {
                Some(target) => run_tool("launchctl", &["bootout", &target]).map(|_| ()),
                None => Ok(()),
            },
        };
        match result {
            Ok(()) => println!("service stopped"),
            // Not loaded / not installed is the normal case for a re-run.
            Err(reason) => println!("note: {reason}"),
        }
    }

    fn after_remove(self) {
        if matches!(self, Self::SystemdUser | Self::SystemdSystem) {
            let _ = self.systemctl(&["daemon-reload"]);
        }
    }

    fn is_running(self) -> bool {
        match self {
            Self::SystemdUser | Self::SystemdSystem => self
                .systemctl(&["is-active", UNIT_NAME])
                .is_ok_and(|out| out.trim() == "active"),
            Self::Launchd => self
                .launchd_target()
                .and_then(|t| run_tool("launchctl", &["print", &t]).ok())
                .is_some_and(|out| out.contains("state = running")),
        }
    }

    fn post_install_hints(self) {
        match self {
            Self::SystemdUser => {
                let user = std::env::var("USER").unwrap_or_default();
                let lingering = run_tool("loginctl", &["show-user", &user, "--property=Linger"])
                    .is_ok_and(|out| out.trim() == "Linger=yes");
                if !lingering {
                    println!(
                        "\nNote: a user service stops when you log out and only starts at boot \
                         with lingering on. To keep tunnels up on a server, run once:\n  \
                         sudo loginctl enable-linger {}",
                        if user.is_empty() { "$USER" } else { &user }
                    );
                }
                println!("Logs: journalctl --user -u tunnel-pilot -f");
            }
            Self::SystemdSystem => println!("Logs: journalctl -u tunnel-pilot -f"),
            Self::Launchd => println!("Logs: {}", launchd_log_path().display()),
        }
    }
}

/// Quote one `ExecStart=` argument for systemd: always double-quoted, with
/// `\` and `"` escaped, `%` doubled (specifier) and `$` doubled (env
/// expansion). Pure.
pub fn systemd_quote(arg: &str) -> String {
    quote_systemd(arg, true)
}

/// Quote an `Environment=` assignment: like [`systemd_quote`], but `$` is left
/// alone — systemd does not expand variables there. Pure.
pub fn systemd_env_quote(assignment: &str) -> String {
    quote_systemd(assignment, false)
}

fn quote_systemd(arg: &str, escape_dollar: bool) -> String {
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for ch in arg.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '%' => out.push_str("%%"),
            '$' if escape_dollar => out.push_str("$$"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// The systemd unit text. Pure — unit-tested.
pub fn systemd_unit(exe: &Path, args: &[String], env: &[(String, String)], system: bool) -> String {
    let exec = std::iter::once(systemd_quote(&exe.display().to_string()))
        .chain(args.iter().map(|a| systemd_quote(a)))
        .collect::<Vec<_>>()
        .join(" ");
    let environment: String = env
        .iter()
        .map(|(k, v)| format!("Environment={}\n", systemd_env_quote(&format!("{k}={v}"))))
        .collect();
    let wanted_by = if system {
        "multi-user.target"
    } else {
        "default.target"
    };
    format!(
        "# Managed by `tunnel-pilot service install` — re-run it to change, \
         `tunnel-pilot service uninstall` to remove.\n\
         [Unit]\n\
         Description=Tunnel Pilot SSH port-forward daemon\n\
         Documentation=https://github.com/kalfian/tunnel-pilot\n\
         Wants=network-online.target\n\
         After=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={exec}\n\
         {environment}\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy={wanted_by}\n"
    )
}

/// Escape text for a plist `<string>`. Pure.
pub fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The launchd agent plist. Pure — unit-tested.
pub fn launchd_plist(exe: &Path, args: &[String], env: &[(String, String)], log: &Path) -> String {
    let program_args: String = std::iter::once(exe.display().to_string())
        .chain(args.iter().cloned())
        .map(|a| format!("    <string>{}</string>\n", xml_escape(&a)))
        .collect();
    let env_entries: String = env
        .iter()
        .map(|(k, v)| {
            format!(
                "    <key>{}</key>\n    <string>{}</string>\n",
                xml_escape(k),
                xml_escape(v)
            )
        })
        .collect();
    let log = xml_escape(&log.display().to_string());
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <!-- Managed by `tunnel-pilot service install`. -->\n\
         <plist version=\"1.0\">\n<dict>\n\
         \x20 <key>Label</key>\n  <string>{LAUNCHD_LABEL}</string>\n\
         \x20 <key>ProgramArguments</key>\n  <array>\n{program_args}  </array>\n\
         \x20 <key>EnvironmentVariables</key>\n  <dict>\n{env_entries}  </dict>\n\
         \x20 <key>RunAtLoad</key>\n  <true/>\n\
         \x20 <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n\
         \x20 <key>StandardOutPath</key>\n  <string>{log}</string>\n\
         \x20 <key>StandardErrorPath</key>\n  <string>{log}</string>\n\
         </dict>\n</plist>\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_quote_escapes_specifiers_env_and_quotes() {
        assert_eq!(systemd_quote("plain"), "\"plain\"");
        assert_eq!(systemd_quote("a b"), "\"a b\"");
        assert_eq!(systemd_quote("100%"), "\"100%%\"");
        assert_eq!(systemd_quote("$HOME"), "\"$$HOME\"");
        assert_eq!(systemd_quote("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(systemd_quote("c:\\x"), "\"c:\\\\x\"");
        assert_eq!(systemd_env_quote("A=$x%"), "\"A=$x%%\"");
    }

    #[test]
    fn systemd_unit_pins_binary_args_and_config_dir() {
        let unit = systemd_unit(
            Path::new("/home/me/.local/bin/tunnel-pilot"),
            &["daemon".into(), "--connect".into(), "prod db".into()],
            &[(
                "TUNNEL_PILOT_CONFIG_DIR".into(),
                "/home/me/.config/com.kalfian.tunnelpilot".into(),
            )],
            false,
        );
        assert!(unit.contains(
            "ExecStart=\"/home/me/.local/bin/tunnel-pilot\" \"daemon\" \"--connect\" \"prod db\"\n"
        ));
        assert!(unit.contains(
            "Environment=\"TUNNEL_PILOT_CONFIG_DIR=/home/me/.config/com.kalfian.tunnelpilot\"\n"
        ));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(unit.contains("Restart=on-failure"));

        let system = systemd_unit(Path::new("/usr/local/bin/tunnel-pilot"), &[], &[], true);
        assert!(system.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn launchd_plist_escapes_and_lists_every_argument() {
        let plist = launchd_plist(
            Path::new("/Users/me/bin/tunnel-pilot"),
            &["daemon".into(), "--connect".into(), "a&b".into()],
            &[("TUNNEL_PILOT_CONFIG_DIR".into(), "/Users/me/<cfg>".into())],
            Path::new("/Users/me/Library/Logs/tunnel-pilot-daemon.log"),
        );
        assert!(plist.contains("<string>com.kalfian.tunnelpilot.daemon</string>"));
        assert!(plist.contains("<string>/Users/me/bin/tunnel-pilot</string>"));
        assert!(plist.contains("<string>a&amp;b</string>"));
        assert!(plist.contains("<string>/Users/me/&lt;cfg&gt;</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(!plist.contains("a&b<"), "raw ampersand must not leak");
    }
}
