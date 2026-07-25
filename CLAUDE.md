# Tunnel Pilot (v2)

Cross-platform SSH local port-forwarding manager (macOS, Windows, Linux). Lives in the
system tray — no Dock icon on macOS by default; the webview is destroyed while the window
is hidden so idle footprint stays small.

**Stack:** Rust + Tauri v2 backend, Svelte 5 + TypeScript + Vite frontend. SSH via
`russh` 0.62 (pure-Rust async on tokio). State persisted as JSON in the OS app-config dir;
passwords in the OS keychain (`keyring`) with a plaintext-fallback file when no keychain is
available. Signed self-updates via `tauri-plugin-updater` (minisign).

> The `spec/` directory is the authoritative contract for this rewrite. This file is the
> at-a-glance brief; deep detail lives in the spec. Start at `spec/00-README.md`, then
> `spec/02-ARCHITECTURE.md` (IPC/event catalog + module tree) and `spec/AGENTS.md`
> (the detailed conventions every agent must follow). `spec/PROGRESS.md` tracks build state.

## Quick Reference

- **Package name**: `com.kalfian.tunnel_pilot`
- **App version**: 2.0.0
- **Toolchain**: Rust via rustup (do not use asdf-rust) · Node + pnpm via asdf

### Dev commands

```bash
pnpm install                 # frontend deps
pnpm tauri dev               # run app (Rust core + Vite HMR)
pnpm tauri build             # release bundle for the current OS

# Frontend (from repo root)
pnpm check                   # svelte-check / tsc
pnpm lint                    # prettier --check + eslint
pnpm test                    # vitest (component/unit)

# Rust (from src-tauri/)
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

## Project Structure

```
src/                                  # Svelte 5 + TS frontend (presentation only)
  main.ts                             # mount app, hydrate on boot, subscribe to events
  App.svelte                          # shell: tab nav (Connections/Logs/Settings) + palette
  app.css
  lib/
    ipc.ts                            # typed wrappers over invoke() — one fn per command (contract)
    events.ts                         # typed listen() subscriptions -> store updates
    types.ts                          # TS types mirroring Rust models (04-DATA-MODEL.md)
    hydrate.ts                        # app_hydrate() reconciliation into stores
    fuzzy.ts                          # command-palette fuzzy match
    validation.ts                     # ForwardForm validation
    components/                       # ConnectionRow/List, ForwardForm, CommandPalette, TrayPopover, ...
    routes/                           # ConnectionsView, LogsView, SettingsView
    stores/                           # forwards, groups, settings, logs, updater, palette, ...
    ui/                               # design-system primitives (Button, Dialog, Input, Menu, Icon, ...)
  styles/                            # tokens.css, base.css

src-tauri/                            # Rust + Tauri v2 core (owns tray, tokio, SSH, state)
  Cargo.toml  tauri.conf.json  build.rs  icons/
  src/
    main.rs                           # entrypoint: builder, plugins, setup, run
    lib.rs                            # shared for integration tests
    error.rs                          # AppError (thiserror), serde-serializable across IPC
    events.rs                         # event name constants + payload structs (Rust -> FE)
    logging.rs                        # tracing setup + LogEntry buffer bridge
    state/                            # AppState, tunnel_registry, settings_state, log_buffer
    ssh/                              # engine (per-tunnel supervisor), client, forward, health, reconnect, wake, stats
    storage/                          # config_file (atomic RMW), migration (v1->v2), backup
    credentials/                      # keychain via keyring; plaintext fallback + warning flag
    tray/                             # dynamic count icon + native menu build/rebuild
    window/                           # hide-on-close, show/focus, single-instance re-show
    platform/                         # dock/activation policy, autostart, notifications
    updater/                          # tauri-plugin-updater wiring; check/download/install
    commands/                         # thin #[tauri::command] handlers: forwards, groups, settings, logs, backup, updater, app

assets/icons/                         # tray icons (idle grey + numbered 1-9), app + menu icons (embedded via include_bytes!)
docs/                                 # landing page (GitHub Pages) + screenshots + install scripts
spec/                                 # full v2 spec package (00-07 + AGENTS.md + design-tokens.md + PROGRESS.md)
```

## Architecture & Conventions

The full rules live in `spec/AGENTS.md`; do not duplicate them here. The essentials:

- **The IPC contract is the source of truth.** Every backend capability is a
  `#[tauri::command]` with a typed wrapper in `src/lib/ipc.ts` and a matching TS type in
  `src/lib/types.ts`. Never call `invoke()` with raw strings from components. Rust structs
  use `#[serde(rename_all = "camelCase")]`; TS types are camelCase. See the command/event
  catalog in `spec/02-ARCHITECTURE.md` §6/§7 — changing the IPC surface means updating the
  Rust command, the `invoke_handler`, `ipc.ts`, `types.ts`, and the §6/§7 tables together.
- **State ownership**: Rust `AppState` is the single source of truth. Svelte stores are
  read-through mirrors kept in sync by events; a window reopen must fully rehydrate via
  `app_hydrate()`. The frontend holds no authoritative tunnel state.
- **SSH engine**: one long-lived per-tunnel supervisor task that owns its russh session and
  loops across reconnect attempts (stable `JoinHandle`). Two-level `CancellationToken`
  (durable parent + per-attempt child); disconnect = cancel parent + await join before
  releasing the port. Liveness comes from russh keepalive + the session-future signal — no
  app-level ping-failure counter. A single shared 3s sampler emits `tunnel://stats`.
- **Guarded state machine**: 5-state status (disconnected/connecting/connected/
  disconnecting/error) written only via `set_status` under the registry lock — supervisor
  owns connecting/connected/error, command handler owns disconnecting/disconnected.
- **Rust style**: tokio only, never block the runtime; no `unwrap()`/`expect()` in prod
  code; `Result<T, AppError>` return types; `tracing` for logs. `cargo fmt` +
  `cargo clippy -D warnings` must pass.
- **Svelte/TS style**: strict mode, no `any`; components are presentational (props in,
  store-writes/events out) and never call `invoke()` directly. Visual details (tokens,
  spacing, states) are owned by the design spec (`spec/05-UI-UX-SPEC.md` +
  `spec/design-tokens.md`).
- **Tray/window**: native tray menu rebuilt on state change; dynamic icon (idle grey / blue
  badge with connection count, clamp 9). macOS uses `LSUIElement=true` with runtime
  activation-policy switching — window shown ⇒ Regular policy (Dock visible), hidden ⇒
  Accessory (tray only).

## Security (non-negotiable)

- **No secrets in config when a keychain is available.** Passwords go to the OS keychain via
  `credentials/`; the config JSON stores only `hasStoredPassword`. Plaintext fallback lives
  in a separate secrets file (with a visible UI warning) only when no keychain exists —
  never in the main config or backups.
- **Never** log, emit over IPC, include in `copy_ssh_command`, or write to backups any
  password. Use placeholders when a value must be referenced (org policy: no credentials in
  output).
- **Signed updates only**: `tauri-plugin-updater` with the embedded minisign public key; the
  private key lives solely in CI secrets. Never weaken signature verification.
- macOS is deliberately **unsandboxed** — do not add App Sandbox without a spec change.

## Branch & Release Model

- Active work is on branch **`rewrite/tauri`**. The Flutter v1 app has been removed; the
  last Flutter build is archived at tag **`v1.4.2-flutter-final`**.
- **Never `git push` from the CLI.** Commit locally; releases are tag-triggered in CI
  (push tag `v2.*` → `tauri-release.yml`). See `spec/06-MIGRATION-REPO.md`.
