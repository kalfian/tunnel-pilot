//! PATH shim install/uninstall for the CLI (spec 03 §20).
//!
//! The app binary *is* the CLI (see [`crate::cli`]), but a `.dmg` install has no
//! install script, so nothing puts `tunnel-pilot` on `$PATH`. This module does
//! it from inside the running app: a symlink named `tunnel-pilot` in a bin
//! directory that is already on `$PATH`, pointing at
//! `std::env::current_exe()` — invoking the app binary through a symlink
//! dispatches CLI mode exactly like invoking it directly.
//!
//! Two candidates, in preference order: `~/.local/bin` (user-writable, no
//! password prompt) then `/usr/local/bin` (usually root-owned ⇒ needs
//! elevation). Elevation is macOS-only (`osascript … with administrator
//! privileges`); on other unixes we hand the user the exact `sudo` command
//! instead of guessing at a helper.
//!
//! Safety rules that must not be relaxed:
//! - **Never delete a regular file.** Only a symlink whose target is ours is
//!   ever removed ([`is_removable_link`]).
//! - **Never install a dev build.** A `current_exe()` under `target/` would
//!   point the user's `$PATH` at a throwaway binary
//!   ([`is_dev_build`]; override with `TUNNEL_PILOT_ALLOW_DEV_SHIM=1`).
//! - **Never prompt for a password unprompted.** The startup hook installs only
//!   when a candidate is writable *without* elevation.
//! - Elevated commands are built with single-quoted, escaped paths
//!   ([`shell_quote`]) — the app path contains a space, and nothing but the
//!   current exe path and the link path is ever interpolated.
//!
//! Everything except the two `#[cfg(unix)]` mutation helpers compiles on
//! Windows (the status simply reports `supported: false`), and unix-only
//! imports are scoped inside those functions so `clippy -D warnings` stays
//! clean on the Windows CI gate.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// The command name we put on `$PATH`.
pub const SHIM_NAME: &str = "tunnel-pilot";

/// Escape hatch: install a shim even though `current_exe()` lives under
/// `target/`. For manual testing of this module only — a dev binary is deleted
/// by the next `cargo clean` and would leave a dangling `$PATH` entry.
pub const ALLOW_DEV_SHIM_ENV: &str = "TUNNEL_PILOT_ALLOW_DEV_SHIM";

/// Message used whenever the platform has no symlink shim story (Windows).
const UNSUPPORTED: &str = "installing the CLI on PATH is only supported on macOS and Linux";

/// Where a shim can live. `UsrLocal` is the system-wide fallback and normally
/// needs an admin prompt; `UserLocal` is the no-elevation happy path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShimTarget {
    /// `~/.local/bin` — user-owned, on `$PATH` for most shells.
    UserLocal,
    /// `/usr/local/bin` — system-wide, on `$PATH` via `/etc/paths`.
    UsrLocal,
}

impl ShimTarget {
    /// The bin directory for this target (`None` only when `$HOME` is unset).
    pub fn dir(self) -> Option<PathBuf> {
        match self {
            ShimTarget::UserLocal => home_dir().map(|h| h.join(".local").join("bin")),
            ShimTarget::UsrLocal => Some(PathBuf::from("/usr/local/bin")),
        }
    }

    /// Short human label, used in CLI output and log lines.
    pub const fn label(self) -> &'static str {
        match self {
            ShimTarget::UserLocal => "~/.local/bin",
            ShimTarget::UsrLocal => "/usr/local/bin",
        }
    }
}

/// One resolved install location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub target: ShimTarget,
    pub dir: PathBuf,
}

impl Candidate {
    /// The symlink path itself (`<dir>/tunnel-pilot`).
    pub fn link_path(&self) -> PathBuf {
        self.dir.join(SHIM_NAME)
    }
}

/// What occupies a candidate path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShimState {
    /// Nothing there.
    Absent,
    /// A symlink resolving to the running binary — up to date.
    LinkedToCurrent,
    /// A symlink pointing somewhere else (an old app location, or a dangling
    /// link left by a moved/deleted install).
    LinkedElsewhere,
    /// A real file (or directory). We never touch it.
    NotASymlink,
}

/// Per-candidate inspection result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShimEntry {
    pub target: ShimTarget,
    /// Absolute path of the shim (`<dir>/tunnel-pilot`).
    pub path: String,
    pub state: ShimState,
    /// Where the symlink points (canonicalized when it resolves, else the raw
    /// link text for a dangling link). `None` unless [`ShimState`] is a link.
    pub linked_path: Option<String>,
    /// The directory exists (or can be created) and we can write in it without
    /// elevation.
    pub writable: bool,
    pub dir_exists: bool,
}

/// The install target picked by [`choose_target`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    pub target: ShimTarget,
    pub path: PathBuf,
    /// The directory is not writable by this user ⇒ an admin prompt is needed.
    pub needs_elevation: bool,
}

/// The full picture the UI and the CLI both render (spec 02 §6.8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliShimStatus {
    /// False on platforms without a symlink shim story (Windows).
    pub supported: bool,
    /// A shim exists at one of the candidates (in any state but `Absent`).
    pub installed: bool,
    /// Path of the shim we report on (first candidate in preference order that
    /// is not `Absent`).
    pub path: Option<String>,
    pub target: Option<ShimTarget>,
    /// The reported shim resolves to the running binary.
    pub links_to_current: bool,
    pub linked_path: Option<String>,
    /// The reported shim is a symlink pointing at something else — stale.
    pub linked_elsewhere: bool,
    /// A real file occupies the reported path; install/uninstall refuse it.
    pub conflict: bool,
    /// Where [`install`] would put the shim when called with `None`.
    pub install_target: Option<ShimTarget>,
    pub install_path: Option<String>,
    /// Installing at `install_target` requires an admin prompt.
    pub needs_elevation: bool,
    pub current_exe: Option<String>,
    /// Install would be refused for being a development build: `current_exe()`
    /// is under `target/` AND [`ALLOW_DEV_SHIM_ENV`] is not set. This mirrors
    /// [`install`]'s own guard on purpose — when the two disagreed, the startup
    /// hook skipped while an explicit install of the same binary succeeded, and
    /// the Settings button rendered disabled with a reason that no longer held.
    pub dev_build: bool,
    /// Every candidate, in preference order — lets the UI surface a stale shim
    /// in the *other* directory.
    pub entries: Vec<ShimEntry>,
}

impl CliShimStatus {
    /// The Windows answer: nothing to inspect, nothing installable.
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            installed: false,
            path: None,
            target: None,
            links_to_current: false,
            linked_path: None,
            linked_elsewhere: false,
            conflict: false,
            install_target: None,
            install_path: None,
            needs_elevation: false,
            current_exe: None,
            dev_build: false,
            entries: Vec::new(),
        }
    }
}

/// `$HOME`, if set and non-empty (same resolution the socket path uses).
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// The install locations, in preference order: no-elevation first.
pub fn candidates() -> Vec<Candidate> {
    [ShimTarget::UserLocal, ShimTarget::UsrLocal]
        .into_iter()
        .filter_map(|target| target.dir().map(|dir| Candidate { target, dir }))
        .collect()
}

/// The running binary, canonicalized so symlink comparisons are meaningful.
pub fn current_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.canonicalize().unwrap_or(exe))
}

/// A `current_exe()` under a Cargo `target/` directory is a dev build: it is
/// deleted by `cargo clean` and rebuilt on every run, so putting it on `$PATH`
/// hands the user a link that breaks silently. Note this also covers the
/// locally built bundle (`target/release/bundle/macos/Tunnel Pilot.app/…`) —
/// deliberately, it is just as throwaway.
pub fn is_dev_build(exe: &Path) -> bool {
    exe.components()
        .any(|c| c.as_os_str() == std::ffi::OsStr::new("target"))
}

/// The [`ALLOW_DEV_SHIM_ENV`] override.
pub fn dev_shim_allowed() -> bool {
    std::env::var(ALLOW_DEV_SHIM_ENV).is_ok_and(|v| v == "1")
}

/// Whether install must be refused for being a development build. Pure in
/// `allowed` so the reporting path ([`shim_status`]) and the acting path
/// ([`install`]) share ONE predicate — they drifted apart once, and the startup
/// hook then skipped a binary that an explicit install accepted.
pub fn dev_build_blocks(exe: &Path, allowed: bool) -> bool {
    is_dev_build(exe) && !allowed
}

/// Can we create a file in `dir` (creating `dir` itself if needed) without
/// elevation? Answered by probing the nearest existing ancestor with a
/// create-and-delete — permission bits alone cannot answer it (`/usr/local/bin`
/// is `rwxr-xr-x root`, which looks writable by mode).
pub fn dir_is_writable(dir: &Path) -> bool {
    match nearest_existing(dir) {
        Some(existing) => probe_write(&existing),
        None => false,
    }
}

/// Walk up to the first path component that exists (the directory we would have
/// to create into).
fn nearest_existing(dir: &Path) -> Option<PathBuf> {
    let mut current = Some(dir);
    while let Some(path) = current {
        if path.as_os_str().is_empty() {
            return None;
        }
        if path.symlink_metadata().is_ok() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

fn probe_write(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let probe = dir.join(format!(
        ".tunnel-pilot-probe-{}-{nanos}",
        std::process::id()
    ));
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Inspect one candidate. Pure with respect to the process (reads only), and
/// takes the exe explicitly so tests can point it at a temp file.
pub fn inspect_candidate(candidate: &Candidate, current_exe: Option<&Path>) -> ShimEntry {
    let path = candidate.link_path();
    let (state, linked_path) = match std::fs::symlink_metadata(&path) {
        Err(_) => (ShimState::Absent, None),
        Ok(meta) if !meta.file_type().is_symlink() => (ShimState::NotASymlink, None),
        Ok(_) => {
            // A dangling link has no canonical form — fall back to the raw link
            // text so a stale shim is still reported (and removable).
            let resolved = std::fs::canonicalize(&path)
                .ok()
                .or_else(|| std::fs::read_link(&path).ok());
            let matches_current = match (&resolved, current_exe) {
                (Some(resolved), Some(exe)) => resolved == exe,
                _ => false,
            };
            let state = if matches_current {
                ShimState::LinkedToCurrent
            } else {
                ShimState::LinkedElsewhere
            };
            (state, resolved.map(|p| p.display().to_string()))
        }
    };

    ShimEntry {
        target: candidate.target,
        path: path.display().to_string(),
        state,
        linked_path,
        writable: dir_is_writable(&candidate.dir),
        dir_exists: candidate.dir.is_dir(),
    }
}

/// Pick where to install: the first candidate we can write to without
/// elevation, else `/usr/local/bin` flagged as needing an admin prompt.
pub fn choose_target(entries: &[ShimEntry]) -> Option<Chosen> {
    let pick = entries
        .iter()
        .find(|e| e.writable)
        .map(|e| (e, false))
        .or_else(|| {
            entries
                .iter()
                .find(|e| e.target == ShimTarget::UsrLocal)
                .map(|e| (e, true))
        })
        .or_else(|| entries.first().map(|e| (e, true)))?;
    let (entry, needs_elevation) = pick;
    Some(Chosen {
        target: entry.target,
        path: PathBuf::from(&entry.path),
        needs_elevation,
    })
}

/// Assemble a status from already-inspected candidates. The reported shim is
/// the first non-`Absent` entry in preference order.
pub fn status_from(entries: Vec<ShimEntry>, exe: Option<&Path>) -> CliShimStatus {
    let reported = entries.iter().find(|e| e.state != ShimState::Absent);
    let chosen = choose_target(&entries);
    CliShimStatus {
        supported: true,
        installed: reported.is_some(),
        path: reported.map(|e| e.path.clone()),
        target: reported.map(|e| e.target),
        links_to_current: reported.is_some_and(|e| e.state == ShimState::LinkedToCurrent),
        linked_path: reported.and_then(|e| e.linked_path.clone()),
        linked_elsewhere: reported.is_some_and(|e| e.state == ShimState::LinkedElsewhere),
        conflict: reported.is_some_and(|e| e.state == ShimState::NotASymlink),
        install_target: chosen.as_ref().map(|c| c.target),
        install_path: chosen.as_ref().map(|c| c.path.display().to_string()),
        needs_elevation: chosen.as_ref().is_some_and(|c| c.needs_elevation),
        current_exe: exe.map(|p| p.display().to_string()),
        dev_build: exe.is_some_and(|e| dev_build_blocks(e, dev_shim_allowed())),
        entries,
    }
}

/// Inspect the real candidates for the running binary.
pub fn shim_status() -> CliShimStatus {
    if !cfg!(unix) {
        return CliShimStatus::unsupported();
    }
    let exe = current_exe();
    let entries = candidates()
        .iter()
        .map(|c| inspect_candidate(c, exe.as_deref()))
        .collect();
    status_from(entries, exe.as_deref())
}

/// May we delete this symlink? Only if it is ours: it resolves to the running
/// binary, or (for a stale/dangling link) it points at a path whose file name is
/// our command name. Anything else belongs to the user.
pub fn is_removable_link(linked: &Path, current_exe: Option<&Path>) -> bool {
    if current_exe.is_some_and(|exe| linked == exe) {
        return true;
    }
    linked.file_name() == Some(std::ffi::OsStr::new(SHIM_NAME))
}

/// Single-quote a value for `/bin/sh`, escaping embedded single quotes the
/// POSIX way (`'` → `'\''`). The app path contains a space, so quoting is not
/// optional — and only the exe/link paths are ever interpolated.
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Quote a shell command as an AppleScript string literal.
pub fn applescript_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Absolute tool paths so the elevated shell does not depend on `$PATH`.
fn path_str<'a>(path: &'a Path, what: &str) -> Result<&'a str, AppError> {
    path.to_str().ok_or_else(|| {
        AppError::InvalidInput(format!("{what} is not valid UTF-8: {}", path.display()))
    })
}

/// `mkdir -p <dir> && ln -sfn <exe> <link>` — the elevated install.
pub fn install_shell_command(exe: &Path, link: &Path) -> Result<String, AppError> {
    let dir = link.parent().unwrap_or(Path::new("/"));
    Ok(format!(
        "/bin/mkdir -p {} && /bin/ln -sfn {} {}",
        shell_quote(path_str(dir, "the install directory")?),
        shell_quote(path_str(exe, "the app path")?),
        shell_quote(path_str(link, "the shim path")?),
    ))
}

/// `rm -f <link>` — the elevated uninstall. The is-a-symlink check has already
/// happened in-process; this only removes what we decided is ours.
pub fn uninstall_shell_command(link: &Path) -> Result<String, AppError> {
    Ok(format!(
        "/bin/rm -f {}",
        shell_quote(path_str(link, "the shim path")?)
    ))
}

/// Run a shell command as an administrator (macOS: one Keychain-backed prompt).
#[cfg(target_os = "macos")]
fn elevate(command: &str) -> Result<(), AppError> {
    use std::process::Command;

    let script = format!(
        "do shell script {} with administrator privileges",
        applescript_quote(command)
    );
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .map_err(|e| AppError::Io(format!("could not run osascript: {e}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let trimmed = stderr.trim();
    // -128 is the AppleScript "user cancelled" code.
    if trimmed.contains("-128") {
        return Err(AppError::InvalidInput(
            "administrator authorization was cancelled".into(),
        ));
    }
    Err(AppError::Io(format!(
        "elevated command failed: {}",
        if trimmed.is_empty() {
            "osascript reported no detail"
        } else {
            trimmed
        }
    )))
}

/// No elevation helper outside macOS: hand back the exact command instead of
/// guessing at `pkexec`/`sudo` availability in a GUI session.
#[cfg(all(unix, not(target_os = "macos")))]
fn elevate(command: &str) -> Result<(), AppError> {
    Err(AppError::InvalidInput(format!(
        "this directory needs root; run it yourself: sudo sh -c {}",
        shell_quote(command)
    )))
}

/// Replace (atomically) the symlink at `link` with one pointing at `exe`.
/// Creating the temp link next to the destination keeps the `rename` on one
/// filesystem, so an existing shim is never briefly missing.
#[cfg(unix)]
fn link_in_place(exe: &Path, link: &Path) -> Result<(), AppError> {
    // Scoped import: the `#[cfg(not(unix))]` build has no `symlink` and a
    // file-level import would fail `clippy -D warnings` on the Windows gate.
    use std::os::unix::fs::symlink;

    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = link.with_file_name(format!(".{SHIM_NAME}.{}.{nanos}.tmp", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    symlink(exe, &tmp)?;
    if let Err(e) = std::fs::rename(&tmp, link) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Install the shim. `target` forces a location; `None` uses [`choose_target`].
/// Returns the refreshed status so a caller never has to guess what happened.
#[cfg(unix)]
pub fn install(target: Option<ShimTarget>) -> Result<CliShimStatus, AppError> {
    let exe = current_exe()
        .ok_or_else(|| AppError::Internal("cannot resolve the running executable path".into()))?;
    if dev_build_blocks(&exe, dev_shim_allowed()) {
        return Err(AppError::InvalidInput(format!(
            "refusing to install a development build ({}) on PATH — it is rebuilt and \
             deleted by cargo. Install the packaged app first, or set {ALLOW_DEV_SHIM_ENV}=1",
            exe.display()
        )));
    }

    let candidates = candidates();
    let entries: Vec<ShimEntry> = candidates
        .iter()
        .map(|c| inspect_candidate(c, Some(&exe)))
        .collect();

    let chosen = match target {
        Some(target) => {
            let entry = entries.iter().find(|e| e.target == target).ok_or_else(|| {
                AppError::InvalidInput(format!("cannot resolve {} (is $HOME set?)", target.label()))
            })?;
            Chosen {
                target,
                path: PathBuf::from(&entry.path),
                needs_elevation: !entry.writable,
            }
        }
        None => choose_target(&entries)
            .ok_or_else(|| AppError::Internal("no install location available".into()))?,
    };

    // A real file in the way is the user's, not ours — never clobber it.
    if entries
        .iter()
        .any(|e| e.target == chosen.target && e.state == ShimState::NotASymlink)
    {
        return Err(AppError::InvalidInput(format!(
            "{} already exists and is not a symlink; remove it yourself first",
            chosen.path.display()
        )));
    }

    if chosen.needs_elevation {
        elevate(&install_shell_command(&exe, &chosen.path)?)?;
    } else {
        link_in_place(&exe, &chosen.path)?;
    }
    tracing::info!(
        shim = %chosen.path.display(),
        exe = %exe.display(),
        elevated = chosen.needs_elevation,
        "CLI shim installed"
    );
    Ok(shim_status())
}

/// Remove every shim of ours from the candidate directories. Idempotent: with
/// nothing installed it just returns the status.
#[cfg(unix)]
pub fn uninstall() -> Result<CliShimStatus, AppError> {
    let exe = current_exe();
    let candidates = candidates();
    let entries: Vec<ShimEntry> = candidates
        .iter()
        .map(|c| inspect_candidate(c, exe.as_deref()))
        .collect();

    let mut blocked: Option<String> = None;
    for entry in &entries {
        let path = PathBuf::from(&entry.path);
        match entry.state {
            ShimState::Absent => continue,
            ShimState::NotASymlink => {
                blocked = Some(entry.path.clone());
                continue;
            }
            ShimState::LinkedToCurrent | ShimState::LinkedElsewhere => {}
        }
        let linked = entry.linked_path.as_deref().map(Path::new);
        // A symlink we cannot attribute to Tunnel Pilot stays put.
        if !linked.is_some_and(|l| is_removable_link(l, exe.as_deref())) {
            blocked = Some(entry.path.clone());
            continue;
        }
        if entry.writable {
            std::fs::remove_file(&path)?;
        } else {
            elevate(&uninstall_shell_command(&path)?)?;
        }
        tracing::info!(shim = %path.display(), "CLI shim removed");
    }

    if let Some(path) = blocked {
        return Err(AppError::InvalidInput(format!(
            "{path} is not a Tunnel Pilot symlink; leaving it untouched"
        )));
    }
    Ok(shim_status())
}

#[cfg(not(unix))]
pub fn install(_target: Option<ShimTarget>) -> Result<CliShimStatus, AppError> {
    Err(AppError::InvalidInput(UNSUPPORTED.into()))
}

#[cfg(not(unix))]
pub fn uninstall() -> Result<CliShimStatus, AppError> {
    Err(AppError::InvalidInput(UNSUPPORTED.into()))
}

/// Startup hook (spec 03 §20): put the CLI on `$PATH` on first run when it is
/// free to do so. Deliberately silent about everything else — an admin password
/// prompt at launch, unprompted, would be hostile, so an elevation-only
/// situation is left for the Settings button. Never fails the caller: startup
/// must not depend on this.
#[cfg(unix)]
pub fn auto_install_on_startup() {
    let status = shim_status();
    let skip = if status.installed {
        Some("a shim already exists")
    } else if status.dev_build {
        Some("this is a development build")
    } else if status.install_target.is_none() {
        Some("no writable install location")
    } else if status.needs_elevation {
        // The admin prompt belongs to an explicit user action, never to launch.
        Some("the install location needs administrator rights")
    } else {
        None
    };
    if let Some(reason) = skip {
        tracing::debug!(reason, "skipping CLI shim auto-install");
        return;
    }
    match install(None) {
        Ok(status) => match status.path {
            Some(path) => tracing::info!(shim = %path, "CLI installed on PATH"),
            None => tracing::warn!("CLI shim install reported no path"),
        },
        Err(e) => tracing::warn!(error = %e, "could not install the CLI on PATH"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Every `fs::` use below sits in a `#[cfg(unix)]` test, so on Windows this
    // import would be dead and `clippy -D warnings` (what the CI gate runs)
    // fails the lib-test target. Same trap as the io traits in `client.rs`.
    #[cfg(unix)]
    use std::fs;

    /// Make `dir` read-only for this user. Returns false when the process can
    /// still write into it (i.e. running as root) so the test can opt out
    /// instead of asserting something untrue.
    #[cfg(unix)]
    fn make_read_only(dir: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o555)).expect("chmod");
        !dir_is_writable(dir)
    }

    fn candidate(target: ShimTarget, dir: &Path) -> Candidate {
        Candidate {
            target,
            dir: dir.to_path_buf(),
        }
    }

    fn entries_for(user: &Path, system: &Path, exe: Option<&Path>) -> Vec<ShimEntry> {
        vec![
            inspect_candidate(&candidate(ShimTarget::UserLocal, user), exe),
            inspect_candidate(&candidate(ShimTarget::UsrLocal, system), exe),
        ]
    }

    #[test]
    fn candidates_prefer_the_user_directory() {
        let candidates = candidates();
        assert!(!candidates.is_empty(), "at least /usr/local/bin resolves");
        if candidates.len() == 2 {
            assert_eq!(candidates[0].target, ShimTarget::UserLocal);
            assert_eq!(candidates[1].target, ShimTarget::UsrLocal);
        }
        assert!(candidates
            .iter()
            .all(|c| c.link_path().file_name() == Some(std::ffi::OsStr::new(SHIM_NAME))));
    }

    #[test]
    fn a_writable_user_dir_wins_and_needs_no_elevation() {
        let user = tempfile::tempdir().expect("tmp");
        let system = tempfile::tempdir().expect("tmp");
        let entries = entries_for(user.path(), system.path(), None);
        let chosen = choose_target(&entries).expect("a target");
        assert_eq!(chosen.target, ShimTarget::UserLocal);
        assert_eq!(chosen.path, user.path().join(SHIM_NAME));
        assert!(!chosen.needs_elevation);
    }

    /// A missing `~/.local/bin` is still the best target when we can create it.
    #[test]
    fn a_missing_but_creatable_dir_counts_as_writable() {
        let home = tempfile::tempdir().expect("tmp");
        let user = home.path().join(".local").join("bin");
        let system = tempfile::tempdir().expect("tmp");
        let entries = entries_for(&user, system.path(), None);
        assert!(entries[0].writable, "creatable dir must count as writable");
        assert!(!entries[0].dir_exists);
        let chosen = choose_target(&entries).expect("a target");
        assert_eq!(chosen.target, ShimTarget::UserLocal);
        assert!(!chosen.needs_elevation);
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_user_dir_falls_through_to_the_system_dir() {
        let user = tempfile::tempdir().expect("tmp");
        let system = tempfile::tempdir().expect("tmp");
        if !make_read_only(user.path()) {
            eprintln!("skipping: this process can write into a 0555 dir (root?)");
            return;
        }
        let entries = entries_for(user.path(), system.path(), None);
        assert!(!entries[0].writable);
        let chosen = choose_target(&entries).expect("a target");
        assert_eq!(chosen.target, ShimTarget::UsrLocal);
        assert!(!chosen.needs_elevation, "the system dir is writable here");
    }

    /// The verified real-world shape: neither directory is writable ⇒
    /// `/usr/local/bin` with an admin prompt.
    #[cfg(unix)]
    #[test]
    fn no_writable_dir_means_usr_local_with_elevation() {
        let user = tempfile::tempdir().expect("tmp");
        let system = tempfile::tempdir().expect("tmp");
        if !make_read_only(user.path()) || !make_read_only(system.path()) {
            eprintln!("skipping: this process can write into a 0555 dir (root?)");
            return;
        }
        let entries = entries_for(user.path(), system.path(), None);
        let chosen = choose_target(&entries).expect("a target");
        assert_eq!(chosen.target, ShimTarget::UsrLocal);
        assert!(chosen.needs_elevation);
        assert_eq!(chosen.path, system.path().join(SHIM_NAME));
    }

    #[test]
    fn choose_target_needs_at_least_one_candidate() {
        assert!(choose_target(&[]).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn status_reports_absent_when_nothing_is_installed() {
        let user = tempfile::tempdir().expect("tmp");
        let system = tempfile::tempdir().expect("tmp");
        let exe = user.path().join("app-binary");
        fs::write(&exe, b"binary").expect("write exe");
        let status = status_from(
            entries_for(user.path(), system.path(), Some(&exe)),
            Some(&exe),
        );
        assert!(status.supported);
        assert!(!status.installed);
        assert!(!status.links_to_current);
        assert!(!status.conflict);
        assert_eq!(status.entries[0].state, ShimState::Absent);
        assert_eq!(status.install_target, Some(ShimTarget::UserLocal));
    }

    #[cfg(unix)]
    #[test]
    fn status_reports_a_correct_link_as_up_to_date() {
        use std::os::unix::fs::symlink;
        let user = tempfile::tempdir().expect("tmp");
        let system = tempfile::tempdir().expect("tmp");
        let exe = system.path().join("Tunnel Pilot.app");
        fs::write(&exe, b"binary").expect("write exe");
        let exe = exe.canonicalize().expect("canonicalize");
        symlink(&exe, user.path().join(SHIM_NAME)).expect("symlink");

        let status = status_from(
            entries_for(user.path(), system.path(), Some(&exe)),
            Some(&exe),
        );
        assert!(status.installed);
        assert!(status.links_to_current);
        assert!(!status.linked_elsewhere);
        assert_eq!(status.target, Some(ShimTarget::UserLocal));
        assert_eq!(status.linked_path, Some(exe.display().to_string()));
    }

    #[cfg(unix)]
    #[test]
    fn status_reports_a_stale_link_even_when_it_dangles() {
        use std::os::unix::fs::symlink;
        let user = tempfile::tempdir().expect("tmp");
        let system = tempfile::tempdir().expect("tmp");
        let exe = system.path().join("current-binary");
        fs::write(&exe, b"binary").expect("write exe");
        let exe = exe.canonicalize().expect("canonicalize");
        let gone = system.path().join("old").join(SHIM_NAME);
        symlink(&gone, user.path().join(SHIM_NAME)).expect("symlink");

        let status = status_from(
            entries_for(user.path(), system.path(), Some(&exe)),
            Some(&exe),
        );
        assert!(status.installed);
        assert!(status.linked_elsewhere);
        assert!(!status.links_to_current);
        assert_eq!(status.linked_path, Some(gone.display().to_string()));
        // Dangling but ours by name ⇒ removable.
        assert!(is_removable_link(&gone, Some(&exe)));
    }

    #[cfg(unix)]
    #[test]
    fn status_reports_a_real_file_as_a_conflict() {
        let user = tempfile::tempdir().expect("tmp");
        let system = tempfile::tempdir().expect("tmp");
        fs::write(user.path().join(SHIM_NAME), b"#!/bin/sh\n").expect("write file");
        let status = status_from(entries_for(user.path(), system.path(), None), None);
        assert!(status.installed);
        assert!(status.conflict);
        assert!(!status.links_to_current);
        assert_eq!(status.entries[0].state, ShimState::NotASymlink);
        assert!(status.entries[0].linked_path.is_none());
    }

    #[test]
    fn a_foreign_symlink_is_never_removable() {
        let exe = PathBuf::from("/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot");
        assert!(is_removable_link(&exe, Some(&exe)));
        assert!(is_removable_link(
            Path::new("/old/path/tunnel-pilot"),
            Some(&exe)
        ));
        assert!(!is_removable_link(
            Path::new("/usr/bin/python3"),
            Some(&exe)
        ));
        assert!(!is_removable_link(Path::new("/usr/bin/python3"), None));
    }

    /// The reported `dev_build` flag and `install`'s guard must agree, or the
    /// startup hook skips a binary that an explicit install would accept (and
    /// the Settings button renders disabled for a reason that no longer holds).
    #[test]
    fn the_override_flows_through_the_shared_dev_build_predicate() {
        let dev = Path::new("/Users/me/dev/tunnel-pilot/src-tauri/target/debug/tunnel-pilot");
        let shipped = Path::new("/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot");
        assert!(dev_build_blocks(dev, false), "dev build blocks by default");
        assert!(!dev_build_blocks(dev, true), "the override unblocks it");
        assert!(
            !dev_build_blocks(shipped, false),
            "a shipped build never blocks"
        );
        assert!(!dev_build_blocks(shipped, true));
    }

    #[test]
    fn dev_builds_are_refused_unless_the_override_is_set() {
        assert!(is_dev_build(Path::new(
            "/Users/me/dev/tunnel-pilot/src-tauri/target/debug/tunnel-pilot"
        )));
        assert!(is_dev_build(Path::new(
            "/Users/me/dev/tunnel-pilot/src-tauri/target/release/bundle/macos/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot"
        )));
        assert!(!is_dev_build(Path::new(
            "/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot"
        )));
        assert!(!is_dev_build(Path::new("/usr/local/bin/tunnel-pilot")));
    }

    /// The elevated command is a shell string, and the real app path contains a
    /// space — quoting is what stops it becoming two arguments.
    #[test]
    fn elevated_commands_quote_every_path() {
        let exe = Path::new("/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot");
        let link = Path::new("/usr/local/bin/tunnel-pilot");
        let cmd = install_shell_command(exe, link).expect("build command");
        assert_eq!(
            cmd,
            "/bin/mkdir -p '/usr/local/bin' && /bin/ln -sfn \
             '/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot' \
             '/usr/local/bin/tunnel-pilot'"
        );
        assert_eq!(
            uninstall_shell_command(link).expect("build command"),
            "/bin/rm -f '/usr/local/bin/tunnel-pilot'"
        );
    }

    #[test]
    fn shell_quoting_survives_a_single_quote() {
        assert_eq!(
            shell_quote("/Users/o'brien/bin"),
            "'/Users/o'\\''brien/bin'"
        );
        assert_eq!(
            applescript_quote("/bin/ln -sfn 'a\\b'"),
            "\"/bin/ln -sfn 'a\\\\b'\""
        );
    }

    #[test]
    fn labels_and_serde_names_are_stable() {
        assert_eq!(ShimTarget::UserLocal.label(), "~/.local/bin");
        assert_eq!(ShimTarget::UsrLocal.label(), "/usr/local/bin");
        let json = serde_json::to_string(&ShimTarget::UserLocal).expect("serialize");
        assert_eq!(json, "\"userLocal\"");
        let json = serde_json::to_string(&ShimState::LinkedToCurrent).expect("serialize");
        assert_eq!(json, "\"linkedToCurrent\"");
    }

    /// The frontend type mirrors these field names (AGENTS §1) — a rename here
    /// silently breaks the Settings UI.
    #[test]
    fn status_serializes_camel_case() {
        let json = serde_json::to_value(CliShimStatus::unsupported()).expect("serialize");
        for key in [
            "supported",
            "installed",
            "path",
            "target",
            "linksToCurrent",
            "linkedPath",
            "linkedElsewhere",
            "conflict",
            "installTarget",
            "installPath",
            "needsElevation",
            "currentExe",
            "devBuild",
            "entries",
        ] {
            assert!(json.get(key).is_some(), "status must carry {key}");
        }
        assert_eq!(json.get("supported").and_then(|v| v.as_bool()), Some(false));
    }
}
