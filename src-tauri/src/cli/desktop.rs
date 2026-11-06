//! `tunnel-pilot desktop install` — upgrade a CLI-only install to the desktop
//! app (spec 03 §21).
//!
//! Deliberately a thin hand-off to the published installer (`install.sh
//! --desktop`), the same script the landing page tells people to run: one
//! install path to maintain, and the platform logic (dmg / AppImage / deb,
//! launcher, .desktop entry) lives in one place. The desktop app reads the same
//! config dir, so every forward and stored password carries over.
//!
//! Integrity matches the landing-page one-liner (HTTPS from GitHub); the
//! desktop app's own later self-updates are minisign-verified.

use crate::cli::client::{EXIT_INTERNAL, EXIT_OK, EXIT_USAGE};

/// The published installer (GitHub Pages, served from `docs/install.sh`).
pub const INSTALL_SCRIPT_URL: &str = "https://kalfian.github.io/tunnel-pilot/install.sh";

/// Run the desktop installer and return its exit code.
pub fn install() -> i32 {
    if cfg!(feature = "gui") {
        println!(
            "This binary already is the desktop edition. To reinstall or update it, run:\n  \
             curl -fsSL {INSTALL_SCRIPT_URL} | bash"
        );
        return EXIT_OK;
    }
    if !cfg!(unix) {
        eprintln!(
            "error: install the desktop app from https://github.com/kalfian/tunnel-pilot/releases"
        );
        return EXIT_USAGE;
    }
    for tool in ["curl", "bash"] {
        if !on_path(tool) {
            eprintln!("error: '{tool}' is required to download the desktop app");
            return EXIT_INTERNAL;
        }
    }

    println!("Installing the Tunnel Pilot desktop app via {INSTALL_SCRIPT_URL}");
    let status = std::process::Command::new("bash")
        .arg("-c")
        .arg(r#"set -o pipefail; curl -fsSL "$1" | bash -s -- --desktop"#)
        .arg("tunnel-pilot-desktop-install")
        .arg(INSTALL_SCRIPT_URL)
        .status();
    let code = match status {
        Ok(s) if s.success() => EXIT_OK,
        Ok(s) => {
            eprintln!("error: the desktop installer exited with {s}");
            s.code().unwrap_or(EXIT_INTERNAL)
        }
        Err(e) => {
            eprintln!("error: could not run the desktop installer: {e}");
            EXIT_INTERNAL
        }
    };

    #[cfg(unix)]
    if code == EXIT_OK && crate::daemon::service::is_installed() {
        println!(
            "\nNote: the background daemon service is still installed. The desktop app and \
             the daemon share one config and should not run at the same time — stop it \
             before opening the app:\n  tunnel-pilot service uninstall"
        );
    }
    code
}

/// Whether `tool` resolves on `$PATH` (no `which` dependency).
fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(tool).is_file()))
}
