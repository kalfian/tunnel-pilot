# Tunnel Pilot - Team Guidance

## Development Standards
- **Strict Semantic Versioning**: Always use clean `vX.Y.Z` format. No build metadata allowed in `pubspec.yaml` or tags.
- **Automated Releases**: GitHub Actions handles tagging and multi-platform builds. Trigger by bumping version in `pubspec.yaml` and pushing to `master`. **Wait for user directive before initiating any release.**
- **Code Style**: Modern desktop aesthetic (Linear/Raycast inspired). SF Pro Text font, 8px/10px/12px border radii.
- **Custom Components**: Use custom toggle widget (36x20), no Material Switches.
- **Status Colors**: Green `#22C55E`, Yellow `#F59E0B`, Red `#EF4444`, Grey `#6B7280`.

## Architecture
- **State Management**: Provider (ChangeNotifier pattern).
- **Storage**: Plain JSON. `Backup exports` MUST exclude passwords but include identity file paths.
- **Window Management**: Always hidden on close (stay in tray). macOS `LSUIElement = true`.

## Testing & Validation
- **Engine**: Always run `flutter test` before proposing PRs or merging.
- **Resilience**: All service inits MUST be wrapped in try-catch to prevent crashes.

Refer to `CLAUDE.md` for project structure and technical quick-references.
