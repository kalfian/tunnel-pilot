# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.4.2] - 2026-06-22

### Fixed
- **Backup Buttons Clickable** — The Export and Import rows now respond to clicks across the entire row, not just on the text/icon.

### Improved
- **Smarter File Picker Defaults** — Export and Import now open in your home folder instead of inheriting `~/.ssh`. The SSH identity-file picker still defaults to `~/.ssh` (and to the home folder on a fresh machine), while letting you browse to any folder.

## [1.4.1] - 2026-06-22

### Improved
- **Monochrome Tray Icon** — Redesigned the menu-bar icon around the app's ring-and-arrow logo as a clean monochrome glyph that adapts to both light and dark menu bars; the active tunnel count now shows as a refined corner badge.

### Fixed
- **Settings Version Label** — Moved the "Tunnel Pilot vX.Y.Z" label to the very bottom of the Settings tab; it previously rendered mid-content between sections.

## [1.4.0] - 2026-06-22

### Added
- **Reorder Tunnels** — Drag tunnels into any order from the Connections list; the order is saved and restored across restarts.
- **New Accent Color** — Refreshed the app, tray icons, and website to a new `#288DCC` brand blue.

### Improved
- **Tray Counter Accuracy** — The tray badge now counts only fully-connected tunnels; a tunnel that's still connecting no longer inflates the count.

### Fixed
- **Tunnel List Stability** — Fixed a rendering exception ("borderRadius on non-uniform border") that spammed the logs while reordering or selecting tunnels, and removed a brief snap-back glitch after dropping a reordered tunnel.

## [1.3.10] - 2026-06-22

### Added
- **SSH Key Picker** — "Browse" now opens directly in `~/.ssh` so hidden key files are reachable, and the identity file path is selection-only to prevent invalid manual entry.

### Improved
- **Refined Desktop UI** — Hover states across rows, toggles, and tabs; removed Material ripple effects for a calmer, native desktop feel.
- **Monospaced Technical Text** — Ports, latency, and timestamps now render in a true monospace font and align in columns.
- **Tray Menu Icons** — Replaced emoji in the tray menu with clean monochrome icons (start, stop, settings, quit, update) matching the native macOS style.
- **Status Color Consistency** — Unified connection status colors with dark-mode-tuned values.

### Fixed
- **Identity File Authentication** — Added the required file-access entitlement so selecting an SSH key no longer fails with an entitlement error.
- **Dark Mode Toggle** — Fixed low-contrast off-state on custom toggles in dark mode.

## [1.3.9] - 2026-06-13

### Fixed
- **Instant Tunnel Usability** — "Connected" status now only fires after SSH authentication completes, eliminating the 10–15s delay where tunnels appeared connected but traffic was rejected.
- **Error Retry in Tray** — Clicking an error-state tunnel now immediately reconnects instead of disconnecting (previously required two clicks to recover).
- **Reconnect Visual Feedback** — UI jumps to "Connecting" (yellow) the moment auto-reconnect fires, instead of remaining red until the SSH attempt begins.
- **Notification Spam** — User-initiated disconnects and in-progress retry attempts are now silent; only final outcomes (connected / all retries exhausted) trigger a notification.
- **Dock Visibility** — Fixed Dock icon not appearing correctly when the window is shown on macOS.

### Improved
- **Tray Error Affordance** — Error-state tunnel items now show "↺ Retry" in the tray menu to clarify that clicking will reconnect, not disconnect.

## [1.3.8] - 2026-05-09

### Fixed
- **Port Conflict Resolution**: Automatically release ports occupied by other active tunnels before connecting, preventing "Address already in use (errno = 48)" errors when switching tunnels on the same port.

## [1.3.7] - 2026-05-09

### Fixed
- **Port Management**: Eliminated "Address already in use" errors by implementing a mandatory port-release wait and a robust retry mechanism (5x) for local socket binding.
- **UI Real-time Sync**: Fixed visual lag where the 'connecting' status (yellow) was not shown immediately in the tray and settings page; UI now provides zero-latency feedback.

### Improved
- **Connection Logic**: Swapped connection order to reserve the local port before initiating SSH handshakes, ensuring faster detection of local port conflicts.

## [1.3.6] - 2026-05-09

### Fixed
- **Manual Disconnect**: Fixed issue where tunnels could get stuck in 'connecting' or 'reconnecting' states; clicking the toggle now reliably forces a disconnect from any active or error state.
- **VPN Detection**: Improved detection of silent connection losses (e.g., when a VPN is disabled) by reducing default heartbeat intervals and ping failure thresholds.

## [1.3.5] - 2026-05-09

### Improved
- **Automated Release**: Triggering a clean automated build and release process.

## [1.3.4] - 2026-05-09

### Improved
- **Compact Tray UI**: Simplified port mapping display by removing the remote host, making the tray menu much narrower and more compact.

## [1.3.3] - 2026-05-09

### Added
- **Mature Port Forwarding**: Significant stability improvements and real-time monitoring of SSH tunnels.
- **Real-time Tunnel Stats**: Live tracking of active connections, uptime, and data throughput (↑Up / ↓Down bytes).
- **Modernized Tray UI**: Redesigned system tray menu with a cleaner layout, intuitive emojis, and native-style colored status icons.

### Fixed
- **UI Redundancy**: Removed duplicate Unicode status indicators in the tray menu.
- **Resilience**: Added robust error handling for corrupted configuration files and fixed potential memory leaks in the SSH lifecycle.

### Improved
- **Performance**: Implemented granular state management (`Selector`) to reduce CPU usage.
- **Tray Experience**: Enhanced port mapping display format and updated bulk action labels.
- **Versioning**: Switched to clean Semantic Versioning (vX.Y.Z) without build metadata.

## [1.2.25] - 2026-05-09

### Added
- **Update Diagnostics**: Detailed error causes and file integrity checks during the update/install process.

### Fixed
- **Stable Update Service**: Comprehensive fixes for update download hangs, memory-efficient sync disk writes, and improved macOS install scripts with rollback support.
- **UI Feedback**: Scrollable error messages and real-time progress for update downloads.

## [1.1.0] - 2026-03-14

### Improved
- **Tray UI**: Initial refinements with native-style status dots and streamlined menu layout.

## [1.0.0] - 2026-03-13

### Added
- **Global Health Monitor**: Near-realtime connection loss detection (3s pings).
- **Optimization**: Significant memory and performance improvements across logging, theme caching, and HTTP clients.

## [0.1.0] - 2026-03-09

### Added
- Initial feature set including multi-platform Tray support (macOS, Windows, Linux).
- Backup & Restore, and Launch at Login functionality.

[1.3.3]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.3.3
[1.2.25]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.2.25
[1.1.0]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.1.0
[1.0.0]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.0.0
[0.1.0]: https://github.com/kalfian/tunnel-pilot/releases/tag/v0.1.0
