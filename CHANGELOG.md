# Changelog

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
