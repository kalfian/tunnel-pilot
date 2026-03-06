# Tunnel Pilot

Cross-platform SSH local port forwarding manager (macOS, Windows, Linux). Lives in the system tray - no Dock icon on macOS.

## Quick Reference

- **Package name**: `com.kalfian.tunnel_pilot`
- **Flutter path**: `/Users/kalfian/Developments/Flutter/flutter/bin/flutter`
- **Run**: `flutter run -d macos`
- **Test**: `flutter test` (57 tests, all passing)
- **Build**: `flutter build macos`

## Project Structure

```
lib/
  main.dart                         # Entry point, window/tray/provider setup
  app.dart                          # MaterialApp with light/dark themes
  models/
    forward_config.dart             # SSH forward config (toJson/fromJson/copyWith)
    app_settings.dart               # Settings: launchAtLogin, notifications, themeMode
    forward_status.dart             # Enum: disconnected/connecting/connected/error
  services/
    ssh_tunnel_service.dart         # SSH connection via dartssh2
    storage_service.dart            # JSON persistence (path_provider)
    tray_service.dart               # System tray icon & menu (dynamic icons)
    notification_service.dart       # Desktop notifications (local_notifier)
    startup_service.dart            # Launch at login (launch_at_startup)
  providers/
    forward_provider.dart           # Forward list + connection status state
    app_settings_provider.dart      # App settings state + theme mode
  screens/
    settings_window.dart            # Main window with Connections/Settings tabs
  widgets/
    forward_list_tile.dart          # Forward row with hover, toggle, status
    forward_form_dialog.dart        # Add/edit forward form dialog
    app_settings_section.dart       # Theme picker, toggles
    backup_restore_section.dart     # Export/import config
assets/icons/                       # Tray icons (idle, active, numbered 1-9), app icons
tunnel-pilot/index.html             # Landing page
```

## Architecture & Patterns

- **State Management**: Provider (ChangeNotifier pattern)
- **Storage**: Plain JSON file via `path_provider` + `dart:io`
- **Theme**: Custom light/dark themes in `app.dart`, accent `#007BFF` (light) / `#3D9AFF` (dark)
- **UI Style**: Modern desktop aesthetic (Linear/Raycast inspired) - custom toggles, grouped cards, no Material switches
- **Window behavior**: Hidden on close (stays in tray). Debug mode shows window, release mode hides on start.
- **Tray icon**: Dynamic - grey when idle, blue `#007BFF` with connection count (1-9) when active

## Key Dependencies

| Package | Purpose |
|---------|---------|
| `system_tray` | System tray icon & context menu |
| `dartssh2` | SSH connections & port forwarding |
| `window_manager` | Window hide/show/close behavior |
| `provider` | State management |
| `local_notifier` | Desktop notifications |
| `launch_at_startup` | Auto-start on login |
| `path_provider` | App support directory |
| `file_picker` | Identity file & backup selection |
| `uuid` | Forward config IDs |

## Platform Config (macOS)

- `Info.plist`: `LSUIElement = true` (no Dock icon)
- `DebugProfile.entitlements`: `network.client`
- `Release.entitlements`: `network.client`, `network.server`, `files.user-selected.read-write`

## Testing

Tests are in `test/` covering models, services, and providers. Run with:
```
flutter test
```

## Common Tasks

### Adding a new setting
1. Add field to `AppSettings` model (with toJson/fromJson)
2. Add getter/setter to `AppSettingsProvider`
3. Add UI row in `AppSettingsSection`

### Adding a new forward config field
1. Add field to `ForwardConfig` (with toJson/fromJson/copyWith/toJsonForBackup)
2. Add form field in `ForwardFormDialog`
3. Update `SshTunnelService.connect()` if needed
4. Update tests

## Conventions

- Font: SF Pro Text (set in theme)
- Border radius: 8px (inputs/buttons), 10px (cards), 12px (dialogs)
- Custom toggle widget (36x20) instead of Material Switch
- Status colors: green `#22C55E`, yellow `#F59E0B`, red `#EF4444`, grey `#6B7280`
- All service inits wrapped in try-catch to prevent crashes
- Backup exports exclude passwords, include identity file paths
