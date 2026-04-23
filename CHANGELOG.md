# Changelog

## 1.2.9 (2026-04-23)

### Improvements

- **SSH Tunnel Lifecycle Hardening** — ServerSocket listen subscription is now stored and cancelled explicitly during cleanup/disconnect/dispose to prevent listener leaks. `keepAliveMaxCount` now controls ping failure tolerance (default: 5 × 3s interval ≈ 15s), so a single network hiccup no longer drops the tunnel. `forwardLocal` now has a 10-second timeout; channels that resolve after the timeout are closed so the SSH session does not leak orphaned channels
- **Dock Visibility Consistency** — The tray "Settings..." entry now respects the `showInDock` setting instead of unconditionally showing the Dock icon. Toggling `showInDock` while the window is open now applies immediately instead of waiting for the next close/open cycle
- **Semver-Compliant Version Comparison** — Update check now handles pre-release suffixes (`-beta`, `-rc.1`) and build metadata (`+sha.abc`) per semver. `1.2.7-beta` is correctly ordered below `1.2.7`, so dev builds get offered the stable release
- **Backup Import Validation** — Backup files are now validated before replacing local configs. Malformed JSON, missing `forwards` field, malformed entries, and backups from newer app versions produce a clear error message instead of a generic cast error
- **Port Range Validation** — The tunnel form now rejects port values outside 1–65535 at submit time rather than failing at connect time

### Fixes

- **Update Download Cleanup** — Partial download files are deleted and response streams drained on both error and cancel paths, preventing HTTP socket leaks and orphaned files in the temp directory

## 1.2.8 (2026-04-23)

### Features

- **Copy SSH Command** — Right-click a tunnel in the list and choose "Copy SSH Command" to copy an equivalent `ssh -N -L ...` command to the clipboard. The command includes the configured bind address, identity file path (quoted if it contains spaces), and SSH port

## 1.2.7 (2026-04-13)

### Fixes

- **Update Flow Stuck** — Fixed update getting stuck at 100% during download/install. Added timeouts to all install operations (prevents indefinite hangs), visible error messages when install fails (instead of silently resetting), status messages during each install step ("Mounting disk image...", "Copying to Applications..."), and a cancel button during download. Also fixed fragile hdiutil mount point parsing

## 1.2.6 (2026-04-13)

### Improvements

- **Unified Release Workflow** — Merged auto-tag and release into a single GitHub Actions workflow, removing PAT token dependency. Release now triggers automatically when pubspec.yaml version is bumped on master

## 1.2.5 (2026-04-13)

### Fixes

- **Auto-Tag Release Trigger** — Fixed auto-tag workflow not triggering the release build. Now explicitly dispatches release workflow via GitHub API instead of relying on tag push events

## 1.2.4 (2026-04-13)

### Improvements

- **Connecting State in Tray** — Tray icon count and tooltip now include tunnels in "connecting" state, not just connected ones. Tooltip shows separate counts (e.g. "2 active, 1 connecting...") and tray menu displays a "connecting" status line

### Fixes

- **Release Dispatch** — Fixed release workflow dispatch configuration

## 1.2.3 (2026-04-01)

### Fixes

- **Connect/Disconnect Race Condition** — Fixed race condition where rapidly toggling a tunnel could cause overlapping connect/disconnect operations. Added `disconnecting` status so the toggle is ignored while a connection or disconnection is in progress
- **Realtime Status Indicators** — Tunnel list now shows a spinner on the status dot during connecting and disconnecting states, with the toggle visually dimmed to indicate it's temporarily disabled. All operations (update, remove, reconnect after sleep, disconnect all) now reflect the `disconnecting` state in realtime before performing the actual disconnect

## 1.2.2 (2026-03-17)

### Fixes

- **Auto Tag Not Triggering Release** — Fixed auto-tag workflow not triggering release workflow. Tags pushed by default `GITHUB_TOKEN` don't trigger other workflows; now uses PAT token so release builds run automatically

## 1.2.1 (2026-03-17)

### Fixes

- **Zombie Tunnel Detection** — Fixed tunnel showing "connected" but all connections through it failing silently. Enabled SSH-level keepalive (uses per-tunnel interval setting) so dead connections are detected by the SSH protocol itself, not just external health checks
- **Forward Failure Recovery** — When port forwarding fails 3 consecutive times (e.g. zombie SSH session), the tunnel now automatically triggers error status and reconnect instead of staying stuck as "connected"

## 1.2.0 (2026-03-14)

### Improvements

- **Tray Menu UI** — Native colored dot icons (green/yellow/red/grey) for tunnel status instead of Unicode characters, matching macOS native menu style (like Herd)
- **Tray Port Info** — Each tunnel now shows port mapping (e.g. `:9201 → :443`) in the tray menu for quick reference
- **Connect All / Disconnect All** — New tray menu actions to connect or disconnect all tunnels at once
- **Better Tray Structure** — Cleaner menu layout with active tunnel count header, grouped sections, and streamlined footer

### Fixes

- **Update Stuck at 100%** — Fixed download progress bar stuck at 100% after download completes. Now shows "Installing..." state during installation, and properly resets UI if install fails

## 1.1.0 (2026-03-14)

### Improvements

- **Tray Menu UI** — Replaced oversized emoji status circles with compact Unicode symbols (●, ◐, ✖, ○) and cleaner label formatting for a more native look

## 1.0.1 (2026-03-13)

### Fixes

- **Update Notification Spam** — Fixed update available notification firing multiple times instead of once per version
- **Auto-Install & Restart** — After downloading an update, the app now automatically installs and restarts instead of just opening the file
  - macOS: mounts DMG, copies .app to /Applications, relaunches
  - Windows: extracts ZIP, runs batch script to replace files and relaunch after exit
  - Linux: extracts tar.gz, runs shell script to replace files and relaunch after exit
  - Falls back to opening the downloaded file if auto-install fails

### Improvements

- **Dependencies Updated** — `window_manager` 0.4.3 → 0.5.1, `launch_at_startup` 0.3.1 → 0.5.1, `file_picker` 8.3.7 → 10.3.10
- **GitHub Actions Node.js 24** — Fixed Node.js 20 deprecation warning by setting `FORCE_JAVASCRIPT_ACTIONS_TO_NODE24` across all CI/CD workflows
- **Test Coverage** — Added tests for `LogService`, `UpdateService`, `SshTunnelService`, `AppSettingsProvider`, and expanded `AppSettings` model tests (27 → 115 total tests)

## 1.0.0 (2026-03-13)

### Improvements

- **Near-Realtime Connection Loss Detection** — Replaced per-tunnel keep-alive timer (30s × 5 failures = ~150s detection) with a single global health monitor that pings every 3 seconds and reports error immediately on first failure (~6 seconds detection time)
  - VPN drop, network loss, or server disconnect now detected within seconds
  - Single `Timer` instance for all tunnels (instead of one per tunnel) — lower memory footprint
  - Uses lightweight `ping()` (~20 bytes) with 3-second timeout
  - Automatic start/stop: timer only runs while tunnels are connected
- **Memory Optimization: Theme Caching** — Light and dark `ThemeData` objects are now created once as `static final` instead of being rebuilt on every widget build cycle
- **Memory Optimization: Log Service** — Pre-compute formatted time and log line strings at creation time instead of on every access; cached `List.unmodifiable()` with dirty flag to avoid re-creating the list on every read
- **Memory Optimization: HTTP Client Reuse** — Update service now reuses a single `HttpClient` instance (lazy singleton) instead of creating a new one per request, with proper cleanup on dispose
- **Memory Optimization: Download Progress Throttling** — Update download progress notifications throttled to 2% increments instead of firing on every chunk, reducing unnecessary UI rebuilds
- **Clean Shutdown** — Added `dispose()` method to `SshTunnelService` for proper timer and connection cleanup on app exit

## 0.1.3 (2026-03-13)

### Features

- **Auto Update** — App now checks for new versions via GitHub Releases API and notifies you when an update is available
  - Configurable auto-check interval (every 6 hours)
  - Download update directly from the app or view the release page in browser
  - Skip version option to dismiss a specific update
  - Platform-specific downloads: DMG (macOS), ZIP (Windows), tar.gz (Linux)
  - System tray menu item when update is available
- **Version Display** — Current app version shown in the bottom-right corner across all tabs

### Improvements

- **Native Notifications (macOS)** — Switched from `osascript` to native `UNUserNotificationCenter` API — notifications now appear as "Tunnel Pilot" instead of "Script Editor"
- **Cross-platform Notifications** — Added `local_notifier` for Windows and Linux desktop notifications
- **Version from pubspec.yaml** — App version is now read from `pubspec.yaml` at runtime via `package_info_plus`, no more hardcoded version constants

## 0.1.2 (2026-03-11)

### Fixes

- **Reconnect After Sleep** — Tunnels that were connected before system sleep are now automatically detected and reconnected when the machine wakes up
  - Uses `WidgetsBindingObserver` to detect app resume after >30 seconds of inactivity
  - Probes each "connected" tunnel to verify it's still alive, and reconnects dead ones

## 0.1.1 (2026-03-09)

### Improvements

- **Tray Click Behavior** — Both left-click and right-click on the tray icon now show the tunnel list context menu instead of opening the settings window
- **Reopen via App Launch** — Running the app again opens the settings window instead of starting a duplicate instance
  - macOS: handled via `applicationShouldHandleReopen` delegate
  - Windows/Linux: single-instance guard using local TCP socket — second instance signals the first and exits
- **Landing Page Responsive** — Added tablet (1024px), mobile (768px), and small mobile (400px) breakpoints for proper responsive layout

### Fixes

- Fixed `install.sh` crash (`auth_header[@]: unbound variable`) caused by empty array expansion under `set -u` — removed unnecessary GitHub auth header for public repo
- Fixed `install.ps1` similarly — removed unused `GITHUB_TOKEN` auth header

## 0.1.0 (2026-03-09)

### Features

- **Right-Click Context Menu** — Right-click on any tunnel in the list to quickly Edit, Duplicate, or Delete
- **Smart Window Reopen** — Clicking the tray icon now opens/focuses the settings window directly (left-click), preventing duplicate windows from opening

### Improvements

- **Tray Click Behavior** — Left-click on tray icon opens the window; right-click shows the context menu
- Removed hover animation on tunnel list items to fix visual glitch

## 0.0.5 (2026-03-09)

### Features

- **Windows & Linux Tray** — System tray now works correctly on Windows and Linux
  - Fixed tray icon not appearing: resolved absolute path issue for `system_tray` on Windows/Linux
  - Bundled pre-built `.ico` files for all tray icons directly in assets (no build-time conversion needed)
- **Desktop Shortcut** — Installers now create desktop and Start Menu shortcuts automatically
  - Linux (`install.sh`): creates `.desktop` file in `~/.local/share/applications/` and `~/Desktop/`
  - Windows (`install.ps1`): creates `.lnk` shortcut on Desktop and in Start Menu → Programs
- **App Icon** — Proper Tunnel Pilot icon on Windows and Linux
  - Windows: `app_icon.ico` (256px, PNG-in-ICO) embedded into `.exe` for taskbar, Alt-Tab, and file explorer
  - Linux: GTK window icon set from bundled `app_icon_256.png` for taskbar and Alt-Tab switcher

### Improvements

- **No Duplicate Close Button on Windows & Linux** — Native title bar / window decorations now hidden
  - Windows: `titleBarStyle: TitleBarStyle.hidden` via `window_manager` removes Win32 title bar
  - Linux: `gtk_window_set_decorated(FALSE)` in native code removes GTK header bar (previously showed native close button alongside the Flutter custom one)
- **Landing Page** — Install section now auto-detects OS and shows the correct tab and install command by default
  - Windows users see **Windows (CMD)** tab + PowerShell command; others see **macOS / Linux / WSL** tab + curl command
  - Windows users can switch to **WSL** command via a pill toggle in the hero install section
- **CI** — Added GitHub API token passthrough to all e2e install jobs to avoid rate limiting on macOS runners
  - Added separate e2e jobs for **macOS**, **Windows (WSL)**, and **Windows (CMD)** install paths

### Fixes

- Fixed `install.sh` referencing non-existent `app_icon.png` (correct name is `app_icon_256.png`)

## 0.0.4 (2026-03-09)

### Features

- **Install Script** — One-command installation for macOS, Linux, and Windows (Git Bash): `curl -fsSL https://kalfian.github.io/tunnel-pilot/install.sh | bash`
  - Auto-detects platform, downloads latest release, installs and launches the app
  - macOS: mounts DMG, copies to `/Applications`, removes quarantine flag
  - Linux: extracts to `~/.local/bin`
  - Windows: extracts to `%APPDATA%/Tunnel Pilot`
- **Windows & Linux Releases** — GitHub Actions now builds and publishes release artifacts for all three platforms (`.dmg`, `.zip`, `.tar.gz`)
- **Launch at Login** — Now enabled by default on fresh installs

### Improvements

- Landing page now shows the install command directly in the hero section with click-to-copy
- Landing page "Download App" button links directly to GitHub Releases

## 0.0.3 (2026-03-09)

### Fixes

- Fixed tunnel stuck in loading/connecting state after an unexpected disconnect followed by manual toggle
- Fixed clicking connect on an error or stuck tunnel causing infinite loading — now force-resets to disconnected before reconnecting
- Added 5 second timeout on SSH socket connection to prevent indefinite loading
- Added generation token system to invalidate stale callbacks from previous connection attempts

## 0.0.2 (2026-03-07)

### Features

- **Logs Tab** — New tab showing connect, disconnect, and error events with timestamps and tunnel names
  - Copy individual log entries or all logs to clipboard
  - Clear all logs button to free up memory
  - Color-coded log levels (info, warning, error)
- **Auto Reconnect** — Automatically retry failed connections with configurable retries and delay
  - Respects user-initiated disconnects (won't auto-reconnect if you manually toggled off)
  - Settings: retry count (default 3) and delay seconds (default 5)
- **SSH Keep-Alive** — Per-tunnel keep-alive settings to detect dead connections
  - Configurable interval (default 30s) and max unanswered count (default 5)
  - Automatically disconnects after too many missed alive messages
- **Show in Dock/Taskbar** — New setting to control Dock visibility (default off)
  - Window always appears in Dock when opened
  - When closed: stays in Dock if enabled, hides if disabled

### Improvements

- **Dock Hiding** — App now properly hides from macOS Dock using `NSApp.setActivationPolicy(.accessory)` and dynamic `skipTaskbar` control
- **SEO** — Landing page optimized for "local port forwarding" search queries with JSON-LD schemas, FAQ section, and Open Graph meta tags
- **Scroll Animations** — Landing page sections now animate on scroll using IntersectionObserver

### Fixes

- Fixed app still showing in Dock despite `LSUIElement` in Info.plist (window_manager was overriding it)
- Fixed close button not hiding app from Dock

## 0.0.1 (2026-03-07)

Initial release of Tunnel Pilot.

### Features

- **System Tray / Menu Bar** — App lives entirely in the menu bar (macOS) with no Dock icon in release builds
- **Quick Toggle** — Turn SSH tunnels on/off directly from the tray menu
- **Connection Status Indicators** — Green (connected), Yellow (connecting), Red (error), White (disconnected)
- **Settings Window** — Manage all tunnel configurations in a clean interface
- **Add / Edit / Duplicate / Delete** — Full CRUD for tunnel configurations
- **Double-click to Edit** — Quick editing of existing tunnels
- **Password & Identity File Auth** — Supports SSH password or identity file authentication
- **Backup & Restore** — Export/import configurations as JSON (passwords excluded for security, identity file paths included)
- **Launch at Login** — Start Tunnel Pilot automatically when you log in
- **Desktop Notifications** — Get notified on connection status changes
- **Custom Window Chrome** — Hidden native title bar with custom close button (hides to tray)
- **Draggable Window** — Custom title bar area for window dragging

### Platform Support

- **macOS** — Tested and working
- **Windows** — Not yet tested
- **Linux** — Not yet tested

### Security

- SSH passwords stored locally only, never included in backup exports
- Identity file paths stored as references and included in backups
