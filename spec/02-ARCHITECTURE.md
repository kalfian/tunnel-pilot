# 02 — Architecture: Tunnel Pilot v2 (Tauri v2 + Rust + Svelte)

> How the app is structured: processes, threads, crate/module layout, the complete IPC
> command catalog, the event catalog, state ownership, and plugin config.
> Cross-refs: [01-PRD.md](01-PRD.md), [03-TECH-SPEC.md](03-TECH-SPEC.md),
> [04-DATA-MODEL.md](04-DATA-MODEL.md), [07-ROADMAP.md](07-ROADMAP.md),
> [AGENTS.md](AGENTS.md).

## 1. High-level model

Tauri v2 gives us two logical sides that communicate over IPC:

- **Core (Rust)** — the process that owns the tray, the tokio runtime, all SSH tunnels,
  persistence, keychain, updater, and app lifecycle. It runs even when the window is
  hidden. **This is where all real work happens.** The window/webview is optional.
- **Frontend (Svelte + TS in the webview)** — pure presentation. It holds no source of
  truth beyond ephemeral UI state; it renders snapshots pushed from Rust and issues
  commands. When the window is hidden the webview may be torn down; the frontend must be
  able to fully rehydrate from Rust on show.

```mermaid
flowchart LR
  subgraph Core["Rust Core (always alive)"]
    RT[tokio runtime]
    ST[AppState<br/>tunnels, configs, settings, stats, logs]
    SSH[SSH engine<br/>per-tunnel tasks]
    HM[Health monitor<br/>shared 3s tick]
    TRAY[Tray icon + menu]
    STORE[Storage<br/>JSON config]
    KC[Keychain / keyring]
    UPD[Updater]
    RT --- SSH
    SSH --- ST
    HM --- ST
    ST --- STORE
    ST --- KC
  end
  subgraph FE["Svelte Frontend (webview, optional)"]
    STORES[Svelte stores]
    UI[Connections / Logs / Settings / Palette]
  end
  UI -- invoke() commands --> ST
  ST -- emit() events --> STORES
  TRAY -- menu events --> ST
  UPD -- signed check/install --> STORE
```

## 2. Process & threading model

- **Single OS process** (`tauri-plugin-single-instance` enforces one). No child processes
  for SSH — `russh` is in-process async.
- **One multi-threaded tokio runtime** owned by the Tauri app, started in `main()` /
  `setup`. All async SSH/network work runs here.
- **Per-tunnel supervisor task**: each tunnel owns **one long-lived** tokio task that owns
  its russh session internally and **loops across reconnect attempts** (so its `JoinHandle`
  identity is stable for the tunnel's life, F21). It holds the accept-loop for local
  connections; each accepted socket spawns a child copy task. It uses a **two-level
  `CancellationToken`** (durable parent + per-attempt child, F6); disconnect = cancel parent +
  await this task's JoinHandle before removing the registry entry (releases the port first).
  It also runs the per-tunnel latency RTT probe (owning the session) and publishes stats into
  a `watch` cell. Replaces v1's generation-counter guard with real structured cancellation
  (see [03 §1/§5](03-TECH-SPEC.md#concurrency)).
- **Single shared stats/latency EMIT sampler**: one `tokio::time::interval(3s)` loop reading
  each tunnel's stats `watch` cell and emitting `tunnel://stats`. It holds **no session** and
  **never tears down** — liveness is owned by russh keepalive + the session-future signal
  (F1). Auto-starts on first connect, stops when no tunnels remain
  (see [03 §2/§6](03-TECH-SPEC.md#keepalive)).
- **Tray & menu** run on the platform's main/event thread (Tauri requirement for UI
  objects). Menu rebuilds are marshalled to the main thread via `AppHandle`.
- **Wake detection**: a low-frequency monotonic-clock watchdog task detects time gaps
  (>30s) implying sleep/resume (see [03 §wake](03-TECH-SPEC.md#wake)).

Communication between the tokio async world and the Tauri/tray main thread goes through
`AppHandle` (clonable, `Send + Sync`) for `emit`/menu updates, and through shared state
guarded by async locks for data.

## 3. Rust crate layout (`src-tauri/`)

```
src-tauri/
  Cargo.toml
  tauri.conf.json
  build.rs
  icons/                     # app + generated icon set
  src/
    main.rs                  # entrypoint: builder, plugins, setup, run
    lib.rs                   # (optional) shared for integration tests
    error.rs                 # AppError (thiserror) + Result alias; serde-serializable for IPC
    state/
      mod.rs                 # AppState struct + accessors, wired as tauri::State
      tunnel_registry.rs     # map<TunnelId, TunnelHandle>: cancellation tokens, stats, status
      settings_state.rs      # in-memory AppSettings mirror
      log_buffer.rs          # ring buffer (cap 500) of LogEntry
    ssh/
      mod.rs
      engine.rs              # long-lived per-tunnel supervisor: connect/reconnect loop, owns session, 5×500ms bind-retry, set_status authority
      client.rs              # russh Handler impl, auth (password/identity), keepalive_interval/keepalive_max
      forward.rs             # local accept loop + direct-tcpip channel piping + byte counters
      health.rs              # shared 3s stats/latency EMIT sampler (reads stats_cell); NO session, NO teardown (F1/F21)
      reconnect.rs           # backoff() helper (loop lives in engine.rs supervisor, F21)
      wake.rs                # sleep/wake watchdog (best-effort; session-future signal is backstop, F15)
      stats.rs               # TunnelStats accounting
    storage/
      mod.rs
      config_file.rs         # atomic read-merge-write of tunnel_pilot_config.json; corruption handling
      migration.rs           # v1 -> v2 detection + import (incl. plaintext pwd -> keychain)
      backup.rs              # export (strip passwords) / import (validate, replace|merge)
    credentials/
      mod.rs                 # keychain via `keyring`; feature-detect; plaintext fallback + warning flag
    tray/
      mod.rs
      icon.rs                # dynamic count icon selection (idle grey / badge 1-9), template on macOS
      menu.rs                # menu build + rebuild-on-change; per-tunnel rows; bulk actions; update notice
    updater/
      mod.rs                 # tauri-plugin-updater wiring; check/download/install; progress events
    window/
      mod.rs                 # hide-on-close intercept; show/focus; single-instance re-show
    platform/
      mod.rs
      dock.rs                # macOS activation-policy; Win/Linux skipTaskbar
      autostart.rs           # tauri-plugin-autostart sync
      notify.rs              # tauri-plugin-notification wrapper; permission timing
    cli/                     # local control socket + CLI client (§9); NOT an IPC command
      mod.rs                 # socket path, TUNNEL_PILOT_SOCKET / _CONFIG_DIR overrides, sun_path guard, EDITION
      protocol.rs            # NDJSON CliRequest/CliResponse (v1), CliData, ForwardView, ForwardFields
      args.rs                # hand-rolled subcommand/flag parser + HELP_TEXT
      server.rs              # #[cfg(unix)] UnixListener: 0700 dir, 0600 socket, accept loop
      service.rs             # request -> ops/engine handlers; resolve_target; wait_for_terminal; CRUD validation
      client.rs              # blocking UnixStream client, renderers, exit codes
      forward_cmd.rs         # add/edit/remove/set-password client flows (secret stored client-side)
      shim.rs                # PATH symlink install/uninstall
    bin/tunnel-pilot-cli.rs  # entry of the server product (feature `server`, 03 §21)
    web/                     # feature `server`: web UI server (§10, 03 §22)
      mod.rs                 # axum router, security headers, CSRF/Origin checks, invoke, WebSocket events, SPA
      auth.rs                # sessions (CSPRNG, idle/absolute expiry, password-change invalidation) + login throttle
      admin.rs               # argon2id admin password file (0600) + `web set-password|status`
      assets.rs              # web UI files embedded by build.rs from ../dist-web
    daemon/                  # #[cfg(all(unix, feature = "server"))] headless AppState owner (03 §21)
      mod.rs                 # `tunnel-pilot-cli daemon [--web]`: boot, bind-first single owner, SIGTERM shutdown
      service.rs             # systemd (user/system) unit + launchd agent install/uninstall/status
    ops/                     # Tauri-free service fns for EVERY command — shared by webview, tray, socket, web
      forwards.rs  groups.rs  settings.rs  logs.rs  backup.rs
      dispatch.rs            # command name + JSON args -> ops (the web transport's binding of §6)
    boot.rs                  # migration + config load shared by the app and the daemon
    rt.rs                    # spawn/block_on on Tauri's runtime (gui) or plain tokio (headless)
    commands/                # ── everything below is #[cfg(feature = "gui")] ──
      mod.rs                 # re-export + invoke_handler list
      forwards.rs            # CRUD, reorder, duplicate, connect/disconnect, retry, copy-ssh-command
      groups.rs              # group/tag CRUD, bulk start/stop, filter helpers
      settings.rs            # get/set settings
      logs.rs                # get logs, clear, copy-all payload
      backup.rs              # export/import commands
      updater.rs             # check/install commands
      app.rs                 # snapshot/hydrate, window show/hide, quit
    events.rs                # event name constants + payload structs (serde) emitted to FE
```

Suggested crate deps (see [AGENTS.md](AGENTS.md) for full policy): `tauri` (v2),
`tokio` (rt-multi-thread, net, time, io-util, sync), `tokio-util` (CancellationToken),
**`russh = "0.45"` + `russh-keys = "0.45"`** (pinned — see [03 Conventions/F16](03-TECH-SPEC.md)),
`serde`/`serde_json`, `thiserror`, `anyhow` (bin edges only),
`tracing` + `tracing-subscriber`, `uuid` (v4), `chrono` or `time`,
`fuzzy-matcher` (palette; optional — palette search may live in FE).

**`keyring` needs per-target features (F9)** — v3 ships no backend by default, so pin them
per OS or `keychain_available()` is always false (full block in [03 §9](03-TECH-SPEC.md#credentials)):
`apple-native` (macOS), `windows-native` (Windows), `sync-secret-service` + `crypto-rust` (Linux).

Plugins: `tauri-plugin-autostart`, `tauri-plugin-notification`, `tauri-plugin-single-instance`,
`tauri-plugin-updater`, **`tauri-plugin-clipboard-manager`** (F14 — the clipboard mechanism for
copy-ssh-command + Logs "Copy All"; do not invent another), `tauri-plugin-dialog` (file picker),
`tauri-plugin-store` (optional; we use custom fs for the main config).

## 4. Svelte frontend layout (`src/`)

```
src/
  main.ts                    # mount app; call app_hydrate() on boot; subscribe to events
  App.svelte                 # shell: tab nav (Connections/Logs/Settings) + palette host
  lib/
    ipc.ts                   # typed wrappers over invoke() — ONE function per command (contract)
    events.ts                # typed listen() subscriptions -> store updates
    types.ts                 # TS types mirroring Rust models (see 04-DATA-MODEL.md)
    stores/
      forwards.ts            # writable<ForwardConfig[]> + status/stats maps
      groups.ts              # groups + active tag filter
      settings.ts            # AppSettings mirror + theme
      logs.ts                # LogEntry[] (mirrors Rust ring buffer via events)
      updater.ts             # update availability + progress
      palette.ts             # command palette open state + query
    components/
      ConnectionRow.svelte
      ConnectionList.svelte
      GroupHeader.svelte
      StatusBadge.svelte
      StatChips.svelte       # bytes/latency/uptime, monospace
      ForwardForm.svelte     # add/edit dialog (keepalive fields, keychain warning)
      ConfirmDialog.svelte
      CommandPalette.svelte  # Cmd/Ctrl+K fuzzy search + actions
      TagFilterBar.svelte
    routes/                  # if using a router; else tab views:
      ConnectionsView.svelte
      LogsView.svelte
      SettingsView.svelte
```

Frontend visual details (design tokens, monospace stack, states) are owned by the design
agent — see [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md). This file only defines data flow and
component responsibilities.

## 5. State ownership

**Source of truth is Rust `AppState`.** Svelte stores are read-through mirrors kept in
sync by events. Never let the frontend hold authoritative tunnel/connection state — a
window reopen must fully rehydrate from Rust.

| Data | Owner (Rust) | Frontend mirror |
|------|--------------|-----------------|
| ForwardConfig list (+ order) | `AppState.configs` (persisted) | `stores/forwards` |
| Live status per tunnel (5-state) | `tunnel_registry`; written ONLY via `set_status` under the registry lock — supervisor owns connecting/connected/error, command handler owns disconnecting/disconnected (F23, [03 §1](03-TECH-SPEC.md#ssh)) | `stores/forwards` (status map) |
| TunnelStats (bytes/latency/uptime/conns) | `stats` per tunnel | `stores/forwards` (stats map) |
| Groups / tags | `AppState.groups` (persisted) | `stores/groups` |
| AppSettings | `settings_state` (persisted) | `stores/settings` |
| Logs (ring buffer, 500) | `log_buffer` (not persisted) | `stores/logs` (via events) |
| Cancellation tokens / task handles | `tunnel_registry` only | — (never crosses IPC) |
| Keychain secrets | `credentials` / OS keychain | never crosses IPC in plaintext |
| Update availability/progress | `updater` | `stores/updater` |
| UI ephemeral (open dialog, palette query, tab) | — | Svelte stores only |

## 6. IPC command catalog (`#[tauri::command]`)

All commands return `Result<T, AppError>` where `AppError` is serde-serializable
(see [error.rs] in §3 and [03](03-TECH-SPEC.md)). Types are defined in
[04-DATA-MODEL.md](04-DATA-MODEL.md). The TS wrappers in `lib/ipc.ts` are the
**contract source of truth** (see [AGENTS.md](AGENTS.md)).

### 6.1 Forwards (`commands/forwards.rs`)

| Command | Signature (Rust args → return) | Triggered by |
|---------|--------------------------------|--------------|
| `list_forwards` | `() -> Vec<ForwardConfig>` | boot / rehydrate |
| `create_forward` | `(input: ForwardInput) -> ForwardConfig` | ForwardForm save (new) |
| `update_forward` | `(id: String, input: ForwardInput) -> ForwardConfig` | ForwardForm save (edit); force-disconnects if connected first |
| `delete_forward` | `(id: String) -> ()` | ConfirmDialog delete |
| `duplicate_forward` | `(id: String) -> ForwardConfig` | row action; name gets " (copy)" |
| `reorder_forwards` | `(ordered_ids: Vec<String>) -> ()` | drag-drop reorder |
| `connect_forward` | `(id: String) -> ()` | toggle on / palette |
| `disconnect_forward` | `(id: String) -> ()` | toggle off / palette (user-initiated → silent) |
| `retry_forward` | `(id: String) -> ()` | click while status=error |
| `start_all` | `() -> ()` | tray "Start All" / palette / keymap — global bulk connect (v1 `connectAll`) |
| `stop_all` | `() -> ()` | tray "Stop All" / palette / keymap — global bulk disconnect (v1 `disconnectAll`) |
| `get_forward_runtime` | `(id: String) -> ForwardRuntime` | on-demand status+stats snapshot |
| `copy_ssh_command` | `(id: String) -> String` | "Copy SSH command" action |
| `set_forward_password` | `(id: String, password: String) -> ()` | form save when password entered (writes to keychain/fallback) |
| `clear_forward_password` | `(id: String) -> ()` | form clears password |

`ForwardInput` = editable subset of `ForwardConfig` (no `id`, no live state); password is
**never** part of it — passwords flow only through `set_forward_password` /
`clear_forward_password`. See [04](04-DATA-MODEL.md).

**Form flow ordering (nit a):** creating a forward WITH a password is a two-call sequence —
(1) `create_forward(input)` returns the new `ForwardConfig` with its generated `id`, then
(2) `set_forward_password(id, password)` using that id. The form must await (1) before (2);
never send the password inside `ForwardInput`. Editing follows the same pattern with the
existing id.

`ForwardRuntime` = `{ status: ForwardStatus, stats: TunnelStats, lastError: Option<String> }`.

### 6.2 Groups & tags (`commands/groups.rs`)

| Command | Signature | Triggered by |
|---------|-----------|--------------|
| `list_groups` | `() -> Vec<TunnelGroup>` | boot / rehydrate |
| `create_group` | `(input: GroupInput) -> TunnelGroup` | new group |
| `update_group` | `(id: String, input: GroupInput) -> TunnelGroup` | rename/recolor |
| `delete_group` | `(id: String) -> ()` | delete group (forwards keep tags, groupId cleared) |
| `assign_forward_group` | `(forward_id: String, group_id: Option<String>) -> ()` | drag into folder |
| `start_group` | `(group_id: String) -> ()` | Start All per group |
| `stop_group` | `(group_id: String) -> ()` | Stop All per group |
| `list_tags` | `() -> Vec<String>` | tag filter bar |

### 6.3 Settings (`commands/settings.rs`)

| Command | Signature | Triggered by |
|---------|-----------|--------------|
| `get_settings` | `() -> AppSettings` | boot / rehydrate |
| `update_settings` | `(input: AppSettings) -> AppSettings` | Settings tab change (applies autostart/dock/theme side effects) |

### 6.4 Logs (`commands/logs.rs`)

| Command | Signature | Triggered by |
|---------|-----------|--------------|
| `get_logs` | `() -> Vec<LogEntry>` | open Logs tab / rehydrate |
| `clear_logs` | `() -> ()` | Clear button |
| `get_logs_text` | `() -> String` | Copy All (formatted lines) |

### 6.5 Backup (`commands/backup.rs`)

| Command | Signature | Triggered by |
|---------|-----------|--------------|
| `export_backup` | `(path: String) -> ()` | export (writes BackupFile, passwords stripped) |
| `import_backup` | `(path: String, mode: ImportMode) -> ImportResult` | import; `mode` = `Replace`\|`Merge`; validates + rejects version>current |
| `export_backup_text` | `() -> String` | the same backup JSON as a string (web UI download, §10) |
| `import_backup_text` | `(content: String, mode: ImportMode) -> ImportResult` | import from uploaded text (web UI, §10) |

`ImportMode` enum `{ Replace, Merge }`; `ImportResult` = `{ imported: usize, skipped: usize, replaced: bool }`.

### 6.6 Updater (`commands/updater.rs`)

| Command | Signature | Triggered by |
|---------|-----------|--------------|
| `check_update` | `() -> UpdateStatus` | manual "Check for updates" / autoCheck on boot |
| `install_update` | `() -> ()` | user accepts; downloads + verifies signature + installs; emits progress |
| `skip_update` | `(version: String) -> ()` | "Skip this version" → sets `lastSkippedVersion` |

`UpdateStatus` = `{ available: bool, version: Option<String>, notes: Option<String>, skipped: bool }`.

### 6.7 App / window (`commands/app.rs`)

| Command | Signature | Triggered by |
|---------|-----------|--------------|
| `app_hydrate` | `() -> AppSnapshot` | frontend boot — one call returns forwards+groups+settings+logs+runtime+update status |
| `show_window` | `() -> ()` | tray "Open" |
| `hide_window` | `() -> ()` | custom close button |
| `quit_app` | `() -> ()` | tray "Quit" / palette |

`AppSnapshot` = `{ forwards, groups, settings, logs, runtimes: Vec<(String, ForwardRuntime)>, update: UpdateStatus, keychain_available: bool }`.
`keychain_available` drives the plaintext-fallback warning UI.

### 6.8 CLI on PATH (`commands/cli_shim.rs`)

| Command | Signature | Triggered by |
|---------|-----------|--------------|
| `cli_shim_status` | `() -> CliShimStatus` | Settings mount / after install |
| `install_cli_shim` | `(target: Option<ShimTarget>) -> CliShimStatus` | Settings "Install CLI" (target `None` = auto) |
| `uninstall_cli_shim` | `() -> CliShimStatus` | Settings "Remove" |

`ShimTarget` enum `{ UserLocal, UsrLocal }` = `~/.local/bin` | `/usr/local/bin`.
`CliShimStatus` = `{ supported, installed, path, target, linksToCurrent, linkedPath,
linkedElsewhere, conflict, installTarget, installPath, needsElevation, currentExe, devBuild,
entries: ShimEntry[] }`; `ShimEntry` = `{ target, path, state, linkedPath, writable,
dirExists }` with `state` ∈ `absent|linkedToCurrent|linkedElsewhere|notASymlink`.

The rules live in `cli/shim.rs` (03 §20) so the commands, the `install-cli` subcommand and
the startup hook cannot drift: never delete a non-symlink, never install a `target/` build,
never elevate without an explicit user action. `install`/`uninstall` are `async` and run on
the blocking pool — an elevated install waits on an `osascript` password prompt.

## 7. Event catalog (Rust → Frontend, via `AppHandle::emit`)

Names are constants in `events.rs`. Payloads are serde structs. Frontend subscribes in
`lib/events.ts` and updates stores. Emit only on change (coalesce stats).

| Event name | Payload | Meaning |
|------------|---------|---------|
| `tunnel://status` | `{ id: String, status: ForwardStatus, lastError: Option<String> }` | status transition |
| `tunnel://stats` | `{ id: String, stats: TunnelStats }` | stats update on the single 3s sampler tick ([03 §6/F4](03-TECH-SPEC.md#stats)) |
| `log://line` | `LogEntry` | new log line appended |
| `log://cleared` | `()` | logs cleared |
| `forwards://changed` | `Vec<ForwardConfig>` | config list mutated (CRUD/reorder/migration) |
| `groups://changed` | `Vec<TunnelGroup>` | groups mutated |
| `settings://changed` | `AppSettings` | settings changed (e.g. theme applied elsewhere) |
| `update://status` | `UpdateStatus` | availability changed |
| `update://progress` | `{ downloaded: u64, total: Option<u64> }` | download progress |
| `window://focus` | `()` | window re-shown (e.g. via single-instance) → FE may refresh |

## 8. Plugins & config

| Plugin | Purpose | Notes |
|--------|---------|-------|
| tray-icon (core in v2) | Tray icon + menu | Dynamic icon + menu rebuild in `tray/`. |
| `tauri-plugin-single-instance` | One process | Register first; on second launch → `show_window` + emit `window://focus`. |
| `tauri-plugin-autostart` | Launch at login | Synced with `launchAtLogin` on every boot (`platform/autostart.rs`). |
| `tauri-plugin-notification` | Desktop notifications | Unified across OS; permission timing in `platform/notify.rs`. **May silently fail on the unsigned macOS build (F5)** — verify in M6. |
| `tauri-plugin-updater` | Self-update with **minisign-signed bundles** | Minisign pubkey in `tauri.conf.json`; endpoint = same GitHub Releases. Updater-bundle signing is separate from (deferred) OS code-signing. See [03 §16](03-TECH-SPEC.md#updater), [06](06-MIGRATION-REPO.md). |
| `tauri-plugin-clipboard-manager` | Clipboard (F14) | Copy SSH command + Logs "Copy All". The one clipboard mechanism — coder must not invent another. |
| `tauri-plugin-dialog` | File picker | Identity file + backup path selection; scope in capabilities. |
| `tauri-plugin-store` (optional) | Small KV | Main config uses custom fs (`storage/`); store only if convenient for trivial prefs. |

`tauri.conf.json` highlights (detailed in [03](03-TECH-SPEC.md) & [06](06-MIGRATION-REPO.md)):
- Window: `visible:false` at startup (starts hidden → tray), `resizable:true`, `minWidth`/`minHeight` set, decorations per design.
- macOS: `LSUIElement=true` (agent app), activation-policy switching at runtime for dock.
- Updater: `pubkey`, `endpoints` → GitHub Releases `latest.json`. **NOTE (found in M0):**
  `tauri-plugin-updater` v2 is **not a bare no-op** — registering it (`Builder::new().build()`)
  panics at launch unless a `plugins.updater` config block exists (`PluginInitialization`:
  it deserializes to a required `Config` struct, so a missing/`null` block errors). Therefore
  M0 ships a minimal block with `pubkey: ""` + a placeholder `endpoints` entry (the pubkey is
  only parsed at verify time, so empty is fine at init). M6 replaces the pubkey with the real
  minisign public key. (There is no `active` field in the v2 plugin config — that was v1.)
- Capabilities (v2 ACL): expose only the commands above to the main window; restrict fs/dialog scopes.

## 9. Local control socket (CLI)

A terminal user or an LLM agent drives the running app from a shell:

```bash
"/Applications/Tunnel Pilot.app/Contents/MacOS/tunnel-pilot" list
```

The binary normally reaches `$PATH` as `tunnel-pilot` through a symlink the app installs
itself (§6.8 / 03 §20) — `~/.local/bin` when it is writable, `/usr/local/bin` with an admin
prompt otherwise. `install-cli` / `uninstall-cli` do the same from a terminal.

**This is a second DRIVER, not a second state owner.** The socket handler resolves a
target and then calls the *same* service functions the tray and the webview use
(`ssh::engine::connect_forward`/`disconnect_forward`, `ops::forwards::run_start_all`/
`run_stop_all`/`create`/`update`/`delete`). Status writes still go through the guarded `set_status`, `tunnel://status`
is still emitted, and the frontend still rehydrates via `app_hydrate()` — so a CLI-driven
connect updates the tray and the window with no extra plumbing.

**§7 is UNCHANGED by this feature** — the CLI adds no event, and the socket adds no
`#[tauri::command]`: driving tunnels from a shell reuses the service functions directly.
The only IPC the CLI feature owns is §6.8, and that is about the PATH shim (a filesystem
concern), not about the socket.

| Aspect | Decision |
|---|---|
| Transport | Unix domain socket, `#[cfg(unix)]` (Windows named pipe would slot in behind the same `serve` shape) |
| Path | `<appConfigDir>/cli/cli.sock`; `TUNNEL_PILOT_SOCKET` overrides on BOTH sides; client also takes `--socket` |
| Permissions | dir `0700` **when we create it** (an existing dir keeps its mode), socket `0600` after bind; no token (see [03 §20](03-TECH-SPEC.md)) |
| Framing | NDJSON — one JSON object per line, each way |
| Binary | The GUI binary itself; `main.rs` dispatches on `argv[1]` before any Tauri code (a leading option followed by a subcommand is a usage error, not a GUI launch). The server product's binary is `src/bin/tunnel-pilot-cli.rs` (feature `server`, 03 §21) — same dispatcher, no GUI fallback |
| Owner | The desktop app (spawned detached in `.setup()`; a bind failure is logged and the app still starts) **or** `tunnel-pilot-cli daemon` (server product, its own socket under `com.kalfian.tunnelpilot.cli`; binds first and exits `1` if that socket is live — one owner per product) |

Request / response (all `camelCase`, protocol `v: 1`):

```jsonc
{ "v": 1, "cmd": "connect", "target": "prod-db", "timeoutMs": 30000, "wait": true, "force": false }
{ "v": 1, "ok": true,  "data": { "forward": { … }, "waited": true, "timedOut": false } }
{ "v": 1, "ok": false, "error": { "kind": "notFound", "message": "no forward matches 'x'" } }
```

`error` is `AppError` serialized verbatim — the same vocabulary the frontend sees. A `v`
other than `1` is rejected with `invalidInput` (which the client maps to exit `4`, message
`unsupported protocol version …`). `force` is `connect`-only and defaults to `false`:
without it a `connected`/`connecting` tunnel is reported, never re-dialed (03 §20).

| Command | Target | Response `data` |
|---|---|---|
| `list` | — | `{ forwards: ForwardView[] }` |
| `status` | id\|name | `{ forward, stats }` |
| `connect` | id\|name | `{ forward, waited, timedOut }` |
| `disconnect` | id\|name | `{ forward, waited, timedOut }` |
| `connect-all` | — | `{ results, succeeded, failed }` |
| `disconnect-all` | — | `{ results, succeeded, failed }` |
| `version` | — | `{ appVersion, protocol }` |
| `add` | — (+ `forward: ForwardFields`) | `{ forward, change: "created" }` |
| `edit` | id\|name (+ `forward`) | `{ forward, change: "updated" }` |
| `remove` | id\|name | `{ forward, change: "removed" }` |
| `password-set` | id\|name | `{ forward, change: "passwordSet" }` — flag only; the CLI stored the secret itself |
| `password-clear` | id\|name | `{ forward, change: "passwordCleared" }` — the owner deletes the secret |

`install-cli` / `uninstall-cli` are **local** subcommands: they operate on the filesystem
through `cli/shim.rs`, never open the socket, and therefore work with the app closed (exit
`3` is impossible for them). A refusal exits `2`, a filesystem failure `1`.

`ForwardView` = config + live runtime, flattened. It carries `hasStoredPassword` (a
boolean) and the identity-file path, and **never a secret**: `ForwardFields` has no password
field, `password-set` moves only the flag, and the CLI writes the secret into the shared
credential store from its own process (03 §20, AGENTS §8). The additions are backwards
compatible, so `v` stays `1`; an older owner answers the new commands with a parse error.

`daemon`, `service`, `web` and `--version` never open the socket as a client
(03 §21).

**Target resolution** (server-side, single source of truth): exact `id` first, then
case-insensitive exact `name`. Zero matches → `notFound`; more than one → `invalidInput`
naming the ambiguity. No fuzzy or prefix matching — an agent must never connect the wrong
tunnel.

Exit codes: `0` ok · `1` internal/IO · `2` usage · `3` app not running · `4` not found or
ambiguous · `5` operation ended in `error` · `6` timed out. `status` exits `0` whenever the
query succeeded; the tunnel state is in the payload.

## 10. Web transport (`tunnel-pilot-cli`, 03 §22)

`tunnel-pilot-cli` serves the **same Svelte UI** over HTTP so a headless host is managed
from a browser. Features are identical to the desktop app; only the transport differs.
The IPC contract (§6/§7) stays the single source of truth: the web transport is a second
binding of the same command names, argument objects, return values and events.

| Aspect | Decision |
|---|---|
| Server | `tunnel-pilot-cli daemon --web [--bind <addr:port>]` (axum), default `127.0.0.1:1855` |
| Assets | `pnpm build:web` output embedded in the binary; served at `/` (SPA fallback to `index.html`) |
| Auth | Admin password (`tunnel-pilot-cli web set-password`, argon2id, `0600` file). Login ALWAYS required, localhost included. `--web` refuses to start without a password; a non-loopback bind is never allowed without one |
| Session | Cookie `tp_session` — `HttpOnly; SameSite=Strict; Path=/` (+ `Secure` when a `--trust-proxy` proxy reports `X-Forwarded-Proto: https`), random 256-bit id, in-memory, 12h idle expiry, cleared on logout / password change / restart |
| CSRF | Every non-GET `/api/*` request must carry `X-Tunnel-Pilot: 1` (a cross-site form cannot set it) on top of `SameSite=Strict`; the WebSocket handshake checks `Origin` == `Host` |
| Brute force | Per-client throttle (IPv6 per /64), counted before argon2; 5 attempts → backing-off lockout (`429`); ≤ 2 concurrent verifications (`503` beyond) |
| Host | `Host` must be an IP literal, `localhost` or a `--public-host` name (`421` otherwise) — DNS-rebinding defence; `X-Forwarded-*` only with `--trust-proxy` |
| Connections | hyper with a 10s header-read timeout, ≤ 256 open connections |
| TLS | Not built in — terminate HTTPS at a reverse proxy (Caddy/nginx). The UI warns when used over plain HTTP from a non-loopback host |

### Endpoints

| Method + path | Auth | Request | Response |
|---|---|---|---|
| `GET /api/session` | none | — | `200 { authenticated: bool, version: string, edition: "cli" }` |
| `POST /api/login` | none (header required) | `{ password }` | `204` + `Set-Cookie`; `401 AppError(unauthorized)`; `429` when rate-limited |
| `POST /api/logout` | session | — | `204`, cookie cleared |
| `POST /api/invoke/<command>` | session + header | the exact args object Tauri `invoke(command, args)` takes (camelCase keys; `{}` when none) | `200` + JSON of the command's return value (`null` for unit) |
| `GET /api/events` | session (cookie) + `Origin` check | WebSocket upgrade | text frames `{ "event": "<§7 name>", "payload": <§7 payload> }`; server closes `4401` when the session ends |

Errors are the serialized `AppError` (`{ "kind": "...", "message": "..." }`) with status:
`notFound` → 404 (also an unknown or desktop-only command), `invalidInput` → 400,
`unauthorized` → 401, rate limit → 429, everything else → 500.

### Command availability in web mode
All §6 commands are available **except** the desktop-only ones, which answer
`404 notFound`: `show_window`, `hide_window`, `quit_app`, `hide_tray_popover`,
`check_update`, `install_update`, `skip_update`, `cli_shim_status`, `install_cli_shim`,
`uninstall_cli_shim`, and the path-based `export_backup` / `import_backup` (a browser cannot
name a server path). Web mode uses `export_backup_text() -> string` and
`import_backup_text({ content, mode }) -> ImportResult` instead (both also registered as
desktop commands, §6.5). `update_settings` applies the settings but skips desktop side
effects (autostart, dock). `app_hydrate` reports the update status as idle.
`set_forward_password` stays write-only: no command returns a secret (AGENTS §8).

### Frontend
`src/lib/transport.ts` exports `invoke(command, args)` and `listen(event, cb)`; at runtime it
picks Tauri (`window.__TAURI_INTERNALS__` present, `src/lib/runtime.ts::isWeb`) or HTTP +
one shared WebSocket. `ipc.ts` / `events.ts` import from it — components never see the
difference. Web-mode rules:
- **Sync:** the socket reconnects with backoff (500ms doubling to 10s, immediately when the
  browser comes back online); a reconnect after a gap re-runs `app_hydrate`
  (`onReconnect`). Close `4401` → back to the login screen, no retry. A handshake that never
  opened checks `GET /api/session` first so an expired session does not retry forever.
- **Auth:** `WebRoot.svelte` gates on `GET /api/session`; `LoginScreen.svelte` (wrong
  password, 429 with `Retry-After`, unreachable, session-ended states); Sign out in
  Settings → Session and in the command palette.
- **Platform swaps:** browser clipboard (`navigator.clipboard`, `execCommand` fallback on
  plain HTTP); identity file = text field for a path ON THE SERVER; backups via download
  (`export_backup_text`) / upload (`import_backup_text`, 5 MB cap).
- **Hidden:** tray popover, window chrome, updater UI, PATH-shim group, launch-at-login /
  show-in-dock (replaced by a Server group with `service install --web`), desktop
  notifications.
- **Notices:** a persistent plain-HTTP warning on non-loopback hosts; keychain wording says
  "on the server".
- Below 440px of content width the Connections filter moves to its own row.
- `pnpm build:web` → `dist-web/` (embedded by the server build); `pnpm dev:web` proxies
  `/api` to `TUNNEL_PILOT_WEB_ORIGIN` (default `127.0.0.1:1855`).
