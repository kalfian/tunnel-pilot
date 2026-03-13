# Changelog

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
