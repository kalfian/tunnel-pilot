# Changelog

All notable changes to this project will be documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.3.3] - 2026-05-09

### Improved
- **Versioning**: Switched to clean Semantic Versioning (vX.Y.Z) without build metadata for cleaner release management.

## [1.3.2] - 2026-05-09

### Fixed
- **UI Redundancy**: Removed duplicate Unicode status indicators (●, ○, ×) in the tray menu to focus on clean, colored status icons.

### Improved
- **Tray Labels**: Enhanced port mapping display format to `(:local -> host:remote)` and updated bulk action labels to "Start/Stop All Tunnels" for better clarity.

## [1.3.0] - 2026-05-09

### Added
- **Mature Port Forwarding**: Significant stability improvements and real-time monitoring of SSH tunnels.
- **Real-time Tunnel Stats**: Live tracking of active connections, uptime, and data throughput (↑Up / ↓Down bytes).
- **Modernized Tray UI**: Redesigned system tray menu with a cleaner layout and intuitive emojis for better navigation.

### Fixed
- **Resilience**: Added robust error handling for corrupted configuration files and fixed potential memory leaks in the SSH lifecycle.

### Improved
- **Performance**: Implemented granular state management (`Selector`) to reduce CPU usage during active traffic.
- **SSH Transport**: Upgraded `dartssh2` to v2.17.1 for enhanced connectivity.
- **Health Monitoring**: Improved ping-based health check for faster dead-session detection.

## [1.2.25] - 2026-05-09
### Improved
- Internal version bump for update verification.

## [1.2.24] - 2026-05-09
### Fixed
- **Storage**: Replaced `IOSink` with `RandomAccessFile.writeFromSync()` to prevent update download hangs in release builds.

## [1.2.20] - 2026-05-08
### Added
- **Diagnostics**: Detailed error causes and file integrity checks during the update/install process.

## [1.1.0] - 2026-03-14
### Improved
- Initial Tray UI refinements with native-style status dots.

## [1.0.0] - 2026-03-13
### Added
- **Global Health Monitor**: Near-realtime connection loss detection (3s pings).
- **Optimization**: Significant memory and performance improvements across logging, theme caching, and HTTP clients.

## [0.1.0] - 2026-03-09
### Added
- Initial feature set including Tray support for macOS, Windows, and Linux.
- Backup & Restore functionality.
- Launch at login support.

[1.3.1]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.3.1
[1.3.0]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.3.0
[1.2.25]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.2.25
[1.2.24]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.2.24
[1.2.20]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.2.20
[1.1.0]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.1.0
[1.0.0]: https://github.com/kalfian/tunnel-pilot/releases/tag/v1.0.0
[0.1.0]: https://github.com/kalfian/tunnel-pilot/releases/tag/v0.1.0
