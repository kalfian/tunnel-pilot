# Changelog

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
