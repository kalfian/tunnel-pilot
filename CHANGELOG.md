# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
