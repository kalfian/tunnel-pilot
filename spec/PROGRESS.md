# Tunnel Pilot v2 — Build Progress Checkpoint

> **Durable checkpoint** (git-committed) so work survives token exhaustion / context
> summarization. On resume: read this file, run `git log --oneline -15` on branch
> `rewrite/tauri`, then continue from "Next action". Granular per-item + commit hashes here;
> high-level milestones also tracked in the harness task list.

## Ground rules (do not violate)
- Branch: **`rewrite/tauri`**. Never touch Flutter (`lib/`, `macos/`, `pubspec.yaml`) — it moves to `legacy/flutter/` only at M7 cutover.
- **Commit per item, locally. NEVER `git push`** — push/release goes through CI (AGENTS.md §2, 06 §1).
- Commit trailer: `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.
- Definition of Done per item = AGENTS.md §7 (spec acceptance + tests + `cargo fmt`/`clippy -D warnings`/`pnpm check`/`pnpm lint` clean).
- Spec is the contract; if an item reveals a spec error, flag + fix the spec in the same change (AGENTS.md §9).

## Verification spikes (not defects — do at their milestone)
- **F16 (M1, FIRST):** confirm russh 0.45 API — `Config.keepalive_interval`/`keepalive_max`, session-future-end = connection-lost, `channel.into_stream()`. No `ping()`/`is_closed()`. Update 03 if API differs.
- **F5 (M6):** verify `tauri-plugin-notification` on an UNSIGNED macOS build; acceptance must not assume it works.
- **F15 (M6):** verify wake detection across real OS sleep; session-future is the backstop.

## Milestone status
| M | Title | Status | Commits |
|---|---|---|---|
| M0 | Scaffold (Tauri+Svelte+CI) | ✅ done | d146c19..7e1c3a2 (10 commits) |
| M1 | SSH core engine (russh) | ✅ done | e2d42e9..560e799 (4 commits) |
| M2 | Reconnect/wake/stats/persistence/keychain | ✅ done | 8214cdb..9901e4d (10 commits) |
| M3 | Tray/window/lifecycle/autostart/dock | ✅ done | 6610ce8..331a2d4 (5 commits) |
| M4 | Full UI parity | ✅ done | P1 `08c2e35` · P2a `1c94b5c..048de6b` · P2b `fcd25ee..85939c7` |
| M5 | UX improvements | ✅ done | Rust `44ef253`,`ffc4f69` · FE `5c95728..1812a43` |
| M6 | Signed updater + notifications | ⬜ pending | — |
| M7 | Packaging + cutover | ⬜ pending | — |

## M0 item checklist (commit per item)
- [x] Toolchain: add Tauri CLI (`pnpm add -D @tauri-apps/cli`), confirm rustup + node/pnpm.
- [x] Scaffold Tauri v2 + Svelte/TS/Vite **in place at repo root** (no nested folder, Flutter untouched): `package.json`, `vite.config.ts`, `tauri.conf.json`, `src/`, `src-tauri/`.
- [x] Rust module tree per 02 §3 (stub modules + `error.rs` AppError w/ thiserror+Serialize, `events.rs` constants for the 02 §7 catalog).
- [x] Svelte layout per 02 §4 (stores/components/routes, real `lib/ipc.ts`, `lib/events.ts`, `lib/types.ts`).
- [x] Register plugins as no-ops: autostart, notification, single-instance, updater, dialog, clipboard-manager. (Updater needs a minimal config block — see below.)
- [x] Window starts hidden; minimal tray icon with Open/Quit.
- [x] `tracing` + tracing→log_buffer layer stub (`logging.rs`).
- [x] CI: GitHub Actions build matrix mac/win/linux (build only, no signing) per 06 §4 (`.github/workflows/tauri-build.yml`, additive — Flutter release.yml untouched).
- [x] `.gitignore`: `target/`, `node_modules/`, `dist/`, secrets, updater private key.
- [x] Verify: `cargo build` + `pnpm build` + `pnpm tauri build` (mac .app+.dmg) succeed; `cargo clippy -D warnings`/`cargo fmt --check`/`pnpm check`/`pnpm lint` clean; release+debug binary launches to tray (window hidden, tracing boot line, no crash). Interactive tray Open/Quit click pending a real desktop session.

### M0 findings / deviations
- **`tauri-plugin-updater` v2 is not a bare no-op**: registering it panics at launch unless a `plugins.updater` config block exists. Fixed with a minimal block (`pubkey:""` + placeholder endpoint); M6 fills the real minisign pubkey. Spec 02 §8 updated (AGENTS.md §9). Commit `7e1c3a2`.
- **pnpm 11.15 build-approval**: esbuild build script must be approved via `allowBuilds: { esbuild: true }` in `pnpm-workspace.yaml`, else `pnpm <script>` fails a pre-run deps check (ERR_PNPM_IGNORED_BUILDS).
- macOS `ActivationPolicy::Accessory` set in setup as an M0 baseline so dev runs also sit dock-less (full `showInDock` switching remains M3).

## M1 item checklist (commit per item)
- [x] **F16 spike**: pinned `russh`/`russh-keys` `0.45`; verified API against 0.45.0 source. Corrected 03-TECH-SPEC (AGENTS.md §9): (1) `is_closed()` DOES exist on `client::Handle`; (2) the session future is a PRIVATE `join` — NOT awaitable → F7 connection-lost is observed by polling `Handle::is_closed()`; (3) publickey auth takes `Arc<keys::key::KeyPair>`, NOT `PrivateKeyWithHashAlg` (later-version type); `auth_*` return `Result<bool>`; `load_secret_key` is blocking → `spawn_blocking`. `keepalive_interval`/`keepalive_max`, `into_stream()`, `channel_open_direct_tcpip` all matched. Commit `e2d42e9`.
- [x] `state/models.rs` + `ssh/stats.rs` + `state/tunnel_registry.rs` + `state/mod.rs` (AppState) + `ssh/reconnect.rs` (backoff). Two-level tokens (F6), stable JoinHandle (F21), lock-guarded `retry_requested` flag + wakeup-only `retry_notify` (F29), guarded `set_status` transition table (F23/F28/F31), StatsInner (F1: no ping counter; F30: dead-channel counter is per-attempt). Commit `95243b7`.
- [x] `ssh/client.rs` (Handler, connect 15s, auth 30s + identity precedence + accepted-bool check), `ssh/forward.rs` (direct-tcpip 10s, byte-counting copy, per-attempt ForwardFailSignal — 3 consecutive → WAKE only, never parent, F26/F27b/F30), `ssh/engine.rs` (single long-lived supervisor F21; cancellation-aware bind→connect→auth F24; 5×500ms EADDRINUSE bind-retry F25; per-attempt reset F27a/F30; is_closed() session-lost poll F7; 5-arm accept loop + 3s RTT/stats probe §6; error PARK w/ flag-based retry F29; conflict handling; connect/disconnect/retry entry points). Temporary debug commands + AppState manage. `async-trait` dep added (russh Handler is `#[async_trait]`). Commit `f03910e`.
- [x] Tests: registry state-machine (lifecycle, ignore-disconnecting, F28/F31/F6/F27c/F29) + in-process russh forwarding-server integration (end-to-end forward+byte counters, session-death→error no-ping-counter F1/F7, dead-channel reconnect F26, teardown-during-connecting fast + reaches disconnected F24/F31, retry reuses supervisor F23). 24 tests pass. Commit `560e799`.
- [x] **Architecture code-review fixes** (F32 MAJOR + F33/F34 MINOR + F35 coverage): non-blocking spawned RTT/wake probe (F32), atomic start-reserve to prevent double supervisors (F33), token-driven attempt reset (F34). Commit `4c0cf4e`. New tests: silent-drop keepalive-timeout, wedged-probe fast disconnect, no-double-bind, same-port conflict, disconnect-while-parked, retry-hammer-never-lost (F35). Commit `eb0d150`. **30 tests pass (stable x4).**

### M1 findings / deviations (spec corrections in 03-TECH-SPEC §Conventions/§1/§2, AGENTS.md §9)
- **russh 0.45 has `is_closed()`** (spec wrongly said it did not). The session future is a private `join` handle inside `client::Handle` and is **not awaitable** — the F7 connection-lost signal is a **poll of `Handle::is_closed()`** (a 1s interval arm in the supervisor `select!`). Design intent unchanged: keepalive is the teardown authority; no app-level ping counter.
- **publickey auth in 0.45 takes a bare `Arc<keys::key::KeyPair>`**, not `PrivateKeyWithHashAlg` (that type is from a later russh). `auth_*` return `Result<bool>` (must check accepted). `load_secret_key` is blocking → `spawn_blocking`.
- Guarded status writes use `watch::Sender::send_replace` (updates the value even with zero live receivers; plain `send` fails-and-doesn't-update).
- Integration tests run in-process (russh 0.45 server harness) — **no external sshd/docker required**; they run in the normal `cargo test`. Real-OS-sleep wake verification is deferred to M6 (F15) per roadmap.

### M1 post-review fixes (architecture code-review, commits `4c0cf4e`/`eb0d150`)
- **F32 (MAJOR):** the RTT/wake probe ran INLINE in the accept-loop `select!`, so a wedged session (`channel_open_session` hanging to the 3s timeout) blocked user-disconnect, `accept`, and the `is_closed()` liveness poll ~3s every 3s. Fixed: probes are now SPAWNED off the loop — the periodic stats probe updates latency in the shared cell for the next publish (guarded by an in-flight flag to avoid pile-up), and the wake probe pokes a `wake_dead` `Notify` on failure (new `select!` arm). `session.disconnect` in teardown is also bounded by `DISCONNECT_TIMEOUT`. Verified by `user_disconnect_during_wedged_probe_is_fast` (<2s, not ~3s).
- **F33 (MINOR):** `connect_forward`'s check-then-insert wasn't atomic → concurrent same-id connects could spawn duplicate supervisors (2nd insert orphaning the 1st → leaked task holding the port). Fixed with `TunnelRegistry::try_begin_start` (reserve-or-reject under lock; `insert` clears the reservation) — at most one supervisor per id even under concurrent M4 drivers.
- **F34 (MINOR):** `mint_fresh_attempt`/`request_retry` now cancel the OUTGOING attempt token before swapping, so a reconnect reaps the previous attempt's forward children token-driven (their `attempt_cancel` arm), not merely via session-disconnect channel errors — `active_connections` isn't left inflated.
- **F35 (coverage):** added the missing acceptance tests, incl. the true keepalive-timeout path via a black-hole relay (silent TCP death, no graceful disconnect) proving F1/F7 works via keepalive, not a received disconnect.

### Known backlog (NIT — tracked, not a defect)
- **Host-key verification (MITM hardening):** `ssh/client.rs` `check_server_key` returns `Ok(true)` (accepts any server key) — deliberate v1 (`dartssh2`) parity, but a carried-forward MITM exposure. Possible future hardening: host-key pinning / TOFU-with-store (known_hosts-style), surfaced in the UI on first connect + mismatch. Not in v2.0 scope; revisit post-cutover.

## M2 item checklist (commit per item)
Phase 1 (credentials + SSH liveness) landed earlier: `8214cdb`,`db190f7` (credential
store — keychain-first + plaintext fallback, F9) and `2bba0c0`,`b26e35b`,`c724c00`,`7564f3c`
(3s stats emit sampler `health.rs` + sleep/wake watchdog `wake.rs`, F15/F36). This session
completed the persistence + migration + AppState-integration phase:
- [x] `storage/config_file.rs`: `ConfigStore` over `tunnel_pilot_config.json` in the single
      canonical `app_config_dir` (F2). Atomic write (tmp+fsync+rename) serialized behind an
      async `Mutex`; read-merge-write (`save_forwards`/`save_settings`/`save_groups` preserve
      siblings); corruption on load → `.corrupted-<ts>` sidecar + defaults, never crash;
      `tokio::fs` (no sync I/O on the async path). `TunnelGroup` model + `AppSettings`
      PartialEq/Eq added. Commit `361f4ed`.
- [x] `credentials`: `CredentialStore::in_memory()` (InMemoryBackend — headless AppState +
      migration tests, no OS keychain/disk) and `fallback_only(path)` (NullBackend — headless
      Linux + deterministic fallback-route tests). Commit `07665fb`.
- [x] `storage/migration.rs`: hardcoded per-OS v1 probe (`v1_config_path_for(os, base)` pure →
      testable for all 3 OSes; macOS bundle id `com.kalfian.tunnelpilot` NO underscore,
      Windows `%APPDATA%\kalfian\Tunnel Pilot\`, Linux None per F17). `migrate_if_needed`
      (idempotent when schema≥2, in-place upgrade of pre-v2 at v2 path, else probe);
      `import` moves plaintext `sshPassword`→credential store (`hasStoredPassword=true`,
      never in the v2 file), `.v1-backup` copy, atomic v2 write. Lenient v1-**backup** parse
      in `storage/backup.rs` (F19: reject `version>2`, no `groups`→`[]`, ignore legacy
      `sshPassword`). Commit `fc65ffe`.
- [x] AppState integration: configs now an ordered `Vec` (array order = display order),
      groups+settings RAM mirrors, `Arc<CredentialStore>`, cached `keychain_available`,
      optional `Arc<ConfigStore>`. `new_hydrated`/`new_headless`; `set/get/delete_password`
      route to the credential store; `upsert_config`/`remove_config`/`set_settings` flush to
      disk fire-and-forget. `HydrateSnapshot` + `debug_hydrate` make the persisted data +
      keychain warning reachable (real `app_hydrate` = M4). lib.rs boot resolves
      `app_config_dir`, runs `migrate_if_needed`→`load()` via `block_on` at the binary edge,
      defaults on error. Commit `9901e4d`.

### M2 findings / deviations
- **configs is a `Vec`, not a `HashMap`** (M1 used a map): spec 04 §9 mandates array order =
  display order, so the RAM mirror preserves insertion order and lookups are a linear scan
  over the handful of configured forwards (`get_config` is not a hot path). No spec change.
- **Mutations persist via a single ordered writer** (F37, was fire-and-forget spawns — see
  post-review below). Sync accessors (`upsert_config` etc.) stay infallible for the temporary
  debug command surface; the real M4 command surface will report persistence failures to the
  user. Errors are logged (never with the secret). No spec change.
- **No spec corrections needed** (AGENTS §9): the hardcoded per-OS paths, `.corrupted-<ts>`
  sidecar, `.v1-backup` copy, and `app_config_dir` single-dir rule were all implemented as
  written in 03 §7/§8 and 04 §9/§10/§12.

### M2 post-review hardening (code-review fixes)
- **F37 (MAJOR):** `persist_forwards`/`persist_settings` were fire-and-forget detached
  `tauri::async_runtime::spawn`s that each snapshotted a section and raced for the config
  store's write lock — two rapid mutations could land out of order, so an older snapshot
  overwrote a newer one → silent data loss / stale config on next boot. Fixed: all
  persistence now flows through ONE ordered writer task (`persist_writer_loop`, spawned in
  `AppState::new_hydrated`) fed by an `mpsc::unbounded_channel`. Mutations enqueue the latest
  full section snapshot; the single consumer serializes writes in enqueue order and coalesces
  each wakeup's backlog to the newest per section (last-write-wins is correct — every save is
  a whole-section write). Mutation accessor signatures are unchanged (M3 callers unaffected).
  Tests `ordered_writer_persists_last_enqueued_snapshot` (burst of 200+ distinct snapshots →
  on-disk == LAST enqueued) and `ordered_writer_coalesces_both_sections_to_latest`.
  - **M4 FOLLOW-UP (explicit):** error handling stays `tracing::error!` for now. User-facing
    surfacing of persist failures is deferred to M4, which introduces the real command surface
    and can add an async save path returning `Result` from the mutation commands. Do not change
    the (sync/void) mutation accessor signatures before then.
- **F38 (MINOR):** the keychain password read ran synchronously on the async auth path
  (`ssh/client.rs` → `AppState::get_password` → `keyring` get), and `keyring` does blocking OS
  calls (macOS Security framework / Linux Secret Service D-Bus) that can stall a tokio worker.
  Added `AppState::get_password_async` wrapping the read in `tokio::task::spawn_blocking`
  (mirroring the identity-key `load_secret_key` already on `spawn_blocking`) and switched the
  password auth branch to it. The sync `get_password` is retained for boot/migration/sync
  commands.
- **F39 (MINOR security):** the plaintext fallback secrets tmp was created with `std::fs::write`
  (default umask ~0644) then chmod'd to 0600 — a brief window where the secret was
  world-readable. Now created atomically at mode 0600 via
  `OpenOptions::mode(0o600).create_new(true)` on unix (stale tmp cleared first); the atomic
  rename preserves 0600. Non-unix behavior unchanged.
- **NIT:** `AppSettings` gained per-field `#[serde(default)]` (correct v1 defaults via shared
  free functions the `Default` impl also uses) so a partial/legacy settings block merges
  field-by-field with defaults instead of the whole struct resetting to `Default`. Tests
  `app_settings_partial_block_merges_with_defaults` + `app_settings_empty_block_equals_default`.

## M3 item checklist (commit per item)
- [x] `tray/icon.rs`: pure `tray_icon_for_count` (idle at 0, badge 1–9, clamp ≥9) unit-tested;
      PNGs embedded via `include_bytes!` (v1 assets reused as placeholders); `update_tray_icon`
      sets the icon + marks it a macOS template. Commit `6610ce8`.
- [x] `tray/menu.rs`: pure `build_menu_model` (per-tunnel rows + status-driven actions incl.
      Retry-on-error, conditional Start/Stop All, update-notice slot) unit-tested (14 tests);
      `build_tauri_menu` renders it; `handle_menu_event` routes clicks to engine/window;
      `spawn_tray_sync` subscribes to `tunnel://status` and rebuilds icon+menu **debounced
      ~100ms**. Commit `6610ce8`.
- [x] `window/mod.rs`: `CloseRequested` → `prevent_close` + hide (app persists in tray);
      `show_window`/`hide_window` (apply dock visibility); `focus_from_second_instance`
      (single-instance re-show + `window://focus`); `quit_app` tears down every live tunnel
      before exit. Commit `b9b9390`.
- [x] `platform/dock.rs`: pure `dock_visible(window_shown, show_in_dock)` truth table
      unit-tested; `apply` = macOS `set_activation_policy(Regular/Accessory)` (no objc FFI,
      F11) / Win+Linux `set_skip_taskbar`; `refresh` reads `showInDock` from AppState. Commit
      `fdf6ab9`.
- [x] `platform/autostart.rs`: `reconcile(app, launch_at_login)` drives the OS autostart
      registration to match the setting (idempotent), called on boot. Commit `fdf6ab9`.
- [x] `commands/forwards.rs`: `start_all`/`stop_all` (F3) — connectAll/disconnectAll parity;
      registered in the invoke_handler; shared `run_*` helpers reused by the tray bulk items.
      Commit `d5b5e83`.
- [x] `lib.rs`: replaced the M0 minimal tray with `tray::setup`; single-instance re-show;
      autostart reconcile; hide-on-close handler; window boots hidden → dock hidden until
      `show_window`. Commit `331a2d4`.
- [x] Verify: `cargo build`, `cargo test` (**89 pass, +21 M3 unit tests**), `cargo clippy
      --all-targets -D warnings`, `cargo fmt --check`, `pnpm check`, `pnpm lint` all clean.
      Interactive tray/close/dock/single-instance/autostart behavior needs a real desktop
      session — PENDING a display (like M0). Only the pure logic is verified here.

### M3 findings / deviations
- **No spec correction needed** (AGENTS §9). Tech-spec section numbers used: tray §10, menu
  §11, single-instance §11-note, autostart §12, dock §13, hide-on-close §14. (Some stub module
  doc-comments referenced older §12/§13 numbering — corrected in-file to match 03-TECH-SPEC.)
- **`ipc.ts` already had `startAll`/`stopAll`** wrappers from the M0 contract scaffold (spec
  02 §6.1), so no frontend change was required for the bulk commands — only the Rust
  `invoke_handler` registration.
- **Tray icons are v1 pre-colored PNGs reused as placeholders** (`assets/icons/tray_icon_*`),
  embedded at compile time. They render correctly on Windows/Linux; on macOS they are set as
  template images per spec (alpha-tinted by menu-bar appearance). Dedicated monochrome macOS
  template art (count digit as a crisp knockout for light+dark menu bars) is a design-agent
  follow-up; the count→asset selection is final.
- **Update-notice slot is present but inert until M6** — `build_menu_model` is always called
  with `update_available=false`, so the `ID_UPDATE` item never renders yet (the click branch
  is a logged no-op). M6 wires the real availability + `install_update`.
- **Menu rebuild runs on the main thread** via `AppHandle::run_on_main_thread` (AppKit
  requirement) from the debounced tokio task; state is gathered off-thread, only the
  icon/menu apply is dispatched.

## M4 Phase 1 — backend command surface + FE contract (commit per item)
- [x] `commands/forwards.rs`: full §6.1 surface — CRUD, reorder, duplicate,
      connect/disconnect/retry, `get_forward_runtime`, `copy_ssh_command` (pure
      `build_ssh_command`, v1-exact token order, always `-p`, identity quoted
      only on a space, never a password), `set/clear_forward_password` (route
      through `CredentialStore`; config holds only `hasStoredPassword`).
      `update_forward` force-disconnects a live tunnel first (v1 parity). Kept
      `start_all`/`stop_all` + their `run_*` helpers (reused by the tray).
- [x] `commands/groups.rs`: §6.2 — CRUD (persists `collapsed`; `delete_group`
      clears `groupId` on its forwards), `assign_forward_group` (rejects an
      unknown target group), per-group `start_group`/`stop_group` (distinct from
      global bulk, AGENTS §1), `list_tags` (derived, sorted, de-duped).
- [x] `commands/settings.rs`: §6.3 — `get_settings` + `update_settings` (persist
      then autostart reconcile + dock refresh + `settings://changed`).
- [x] `state/log_buffer.rs` + `logging.rs`: real 500-cap newest-first `LogBuffer`
      (push/snapshot/clear/formatted text, emits `log://line`/`log://cleared`);
      `LogBufferLayer` forwards OUR crate's INFO/WARN/ERROR events (extracts
      `message` + `tunnel` field) into the buffer. Buffer is a process-global
      `Arc` shared with `AppState` (created before tracing init in `lib.rs`).
- [x] `commands/logs.rs`: §6.4 — `get_logs`/`clear_logs`/`get_logs_text`.
- [x] `storage/backup.rs` + `commands/backup.rs`: §6.5 — `export_backup`
      (password-stripped) + `import_backup` via pure `plan_import` (replace|merge,
      natural-key dedupe on merge; version>current rejected in `parse_backup`).
      A REPLACE stops live tunnels first (no orphaned bound ports).
- [x] `commands/updater.rs`: §6.6 — `skip_update` real (sets `lastSkippedVersion`);
      `check_update`/`install_update` are M6-deferred stubs (surface complete).
- [x] `commands/app.rs`: §6.7 — `app_hydrate` returns the full `AppSnapshot`
      (forwards+groups+settings+logs+live runtimes+update+keychainAvailable);
      `show_window`/`hide_window`/`quit_app` wrap `window/`.
- [x] **F37 (M2 follow-up done):** mutation commands return `Result` and AWAIT
      the ordered writer's outcome. `PersistMsg` now carries a per-message ack;
      the single writer sends the real save result to every coalesced ack — the
      ordering guarantee is untouched, but a persist failure now propagates to
      the UI as `AppError::Storage`. Sync RAM mutators (`upsert_config` etc.,
      used by headless it_tests) no longer auto-enqueue; commands drive
      `persist_forwards/settings/groups().await?` explicitly. `AppError: Clone`.
- [x] `lib.rs`: register the entire §6 surface in one `invoke_handler`; removed
      the temporary M1 debug commands (`commands/debug.rs`).
- [x] FE contract (`src/lib/types.ts`/`ipc.ts`/`events.ts`) verified complete +
      1:1 with the Rust models/commands/events (was scaffolded at M0; no change
      needed — every §6 command, §7 event, and §04 model is present and matches).
- [x] Capabilities (`capabilities/default.json`) already scope the needed plugin
      permissions; app-defined `#[tauri::command]`s are not ACL-gated in Tauri v2,
      so no per-command permission entries are required (AGENTS §8).
- [x] Gates: `cargo build`, `cargo test` (**106 pass**, +17 over M3), `cargo clippy
      --all-targets -D warnings`, `cargo fmt --check`, `pnpm check`, `pnpm lint`
      all clean.

### M4 Phase 1 findings / deviations
- **No spec correction needed** (AGENTS §9). Names/signatures follow 02 §6 exactly
  (settings command is `update_settings`, not the "set_settings" wording in the
  task brief; logs include `get_logs_text`; `app_hydrate` returns `AppSnapshot`
  WITH the `update` field per 04 §8 even though the brief's prose omitted it —
  the FE type requires it).
- **Updater `check_update`/`install_update` deferred to M6** (signed-bundle flow,
  spec 03 §16): registered + typed now so the surface + ACL are complete;
  `install_update` returns a clear `AppError::Updater` rather than a silent no-op.
- **F38 reuse:** `set/clear_forward_password` credential writes run on
  `spawn_blocking` (blocking keyring calls) via new `set/delete_password_checked`.

## M4 Phase 2b — UI layer (ui-ux, commits `fcd25ee..85939c7`)
Built the entire visual layer on the Phase-2a stores/contract. Design-agent owned
`.svelte` + `src/main.ts` + CSS only; reserved files (stores/hydrate/validation/
ipc/events/types/Rust) consumed as-is.
- [x] **Design tokens** (`src/styles/tokens.css`) transcribed verbatim from
      `design-tokens.md` (`:root` + `[data-theme=dark]`), + `base.css`
      (reset/focus-ring/reduced-motion), imported from `app.css`.
- [x] **UI primitives** (`src/lib/components/ui/*`): Button, Toggle (36×20 custom,
      disabled while pending), Input (string-controlled so number inputs don't
      coerce), Select, SegmentedControl, StatusDot, StatChip, EmptyState, Skeleton,
      SidebarItem, Dialog (focus-trap/Esc/return-focus), Menu, ToastHost, inlined
      Lucide `Icon` (no emoji, no runtime dep). UI plumbing in `src/lib/ui/*`
      (theme/platform/view/toast/format) — NEW files, distinct from reserved data
      stores.
- [x] **App shell** (`App.svelte`): sidebar-rail IA + active-count badge +
      compact rail < 640px; macOS custom transparent titlebar + drag region
      (native decorations on Win/Linux); keychain-fallback banner; 150ms
      reduced-motion-aware route crossfade. `main.ts` boots
      `initTheme()`+`subscribeEvents()`+`hydrateAll()`.
- [x] **Connections**: flat reorderable list (drag + `⌥↑/↓`, optimistic), 5-state
      cards, live mono stat chips, per-row copy-ssh/edit/duplicate/delete + error
      strip Retry/View-log, toolbar (count/filter/dup/del/Add), honest empty/
      filter/loading states.
- [x] **ForwardForm**: General/Advanced sub-tabs (state preserved), v1 fields +
      defaults, password/identity segmented auth + `~/.ssh` picker, `validateForwardForm`
      gating + inline errors + dirty tracking + discard-confirm; password only via
      set/clear_forward_password.
- [x] **Activity**: reverse-chron mono stream, level+substring filters, click-copy/
      Copy-all/Clear (clipboard-manager), empty state.
- [x] **Settings**: all toggles via `updateSettings` (store-driven auto-revert),
      animated reconnect sub-options, 3-icon theme control, six-state update
      banner (inert pending M6), backup export/import + mode + confirm.
- [x] Tests: +10 Vitest component tests (jsdom, `lib/ipc` mocked) — ConnectionRow
      states/toggle, ForwardForm gating/errors/password-channel, Settings import
      mode. **36 FE tests pass.**
- [x] Gates: `pnpm check` (0/0/0), `pnpm lint`, `pnpm test` (36), `pnpm build` all clean.

### M4 Phase 2b findings / deviations (AGENTS §9)
- **No `05`/`design-tokens` spec corrections needed** — tokens implemented verbatim,
  screens/states/keyboard-map followed. Two honest deviations forced by the M4
  backend surface (not spec errors):
  1. **Logs "Clear" has no 4s undo toast** (`05 §5`): `clear_logs` wipes the
     authoritative Rust buffer and there is no restore command, so a real undo is
     impossible in M4 — a plain "Logs cleared" confirmation is shown instead
     (faking undo would desync from the source of truth). Revisit if a restore
     command is added.
  2. **Import confirm shows the mode's effect, not exact "N imported / M overwritten"
     counts** (`05 §6`): there is no dry-run IPC, so counts aren't knowable before
     applying; the real `ImportResult` counts are reported in the post-import toast.
- **Out of M4 scope (M5), left as stubs:** command palette (⌘K), groups/tags UI
  (`GroupHeader`/`TagFilterBar`/`CommandPalette` still comment-only), Wide detail
  pane/sparkline (v2.1). Flat list built per brief.
- **Minor polish deferred (not blocking):** Activity "N new" jump-to-top pill;
  dedicated `Tooltip` component (native `title` used for now); multi-select delete
  variant (single-selection only).
- **Icons inlined** as Lucide SVG path data in `Icon.svelte` (ISC) rather than a
  `lucide-svelte` dependency — keeps the bundle hermetic; one visual weight,
  `currentColor`.
- **Tooling note:** added dev deps `jsdom` + `@testing-library/{svelte,jest-dom,
  user-event}` and the Svelte plugin to `vitest.config.ts` for component tests.
  `pnpm install` normalized `pnpm-lock.yaml`; prettier stayed 3.9.6 but reformatted
  `src/lib/hydrate.test.ts` (whitespace only — required for `pnpm lint`).
- **Anti-slop self-score (05 §16 + agent rubric): 1/10.** Dense single-column list
  (no card grid), flat surfaces + hairlines (no gradients/glass), Lucide icons (no
  emoji), 4/8 spacing tokens, mono tabular numerics, full 5-state machine, honest
  empty/loading/error separation, keyboard-driveable, reduced-motion honored.
- **Needs a real desktop session to verify:** macOS transparent titlebar + traffic-
  light inset + drag region, native decorations on Win/Linux, file-picker/clipboard
  plugin round-trips, live `tunnel://stats` 3s cross-fade, theme follow of OS
  `prefers-color-scheme`, and 560px-min responsive layout. Logic + component tests
  are green headless.

## M5 — UX improvements (N1–N4, N7 + M4-review FE fixes)
Backend (parallel coder): `44ef253` emit `window://focus` on every show path (F44);
`ffc4f69` resizable window + min inner size 560×480 in `tauri.conf.json` (N2 enable).
Frontend (ui-ux, `5c95728..1812a43`):
- [x] **M4-review FE fixes:** F45 (`await subscribeEvents()` before `hydrateAll()` —
      no dropped mid-hydrate event); F46 (validation now requires ≥1 auth method,
      not just exclusivity); F47 (password-store failure after a successful create
      reports "created, but password not saved", not "Save failed"); F48 (Activity
      `{#each}` keyed by content+occurrence, not array index).
- [x] **N1 command palette (⌘K):** pure DP fuzzy matcher `lib/fuzzy.ts` (word-boundary/
      consecutive/multi-term AND, unit-tested); `stores/palette.ts` (open/query/recents)
      + `stores/commands.ts` bus (palette → ConnectionsView dialogs); `CommandPalette.svelte`
      (context-aware connect/disconnect, per-tunnel action sub-menu via →, start/stop all,
      per-group start/stop, jump-to-view, toggle theme, check updates, add, about);
      combobox+listbox a11y, hover/keyboard share one active index; App wires ⌘K + a
      persistent rail search cue; ⌘N global via the bus.
- [x] **N3 groups/tags UI:** collapsible `GroupHeader` (collapse persisted via
      `update_group`), X/Y active count, per-group Start/Stop all, ambient accent rail;
      Ungrouped section; flat list when no groups; `TagFilterBar` bound to `activeTag`
      (auto-pruned), tag pills on cards (3 + "+N"), Group Select + type-to-create Tags
      editor in the form.
- [x] **F43 reorder-under-filter fix:** `ConnectionList` owns filtering and always
      reorders on the FULL ordered id list (keyboard swap stays within a group; drag
      across groups reassigns via `assign_forward_group`); reorder disabled while any
      filter/tag is active; no-move drag no longer persists.
- [x] **N2 responsive reflow:** content area is a CSS container (excludes rail); list
      caps 720 + centers past ~1100; Compact (<640) tightens padding, shrinks filter,
      collapses Duplicate/Delete into a ⋯ overflow menu; stat chips wrap.
- [x] **N7 non-destructive import merge:** VERIFIED already surfaced — `importMode`
      defaults to `merge`, Merge/Replace segmented control, pre-apply confirm dialog
      with mode-specific copy, post-import result counts. No change needed.
- [x] Tests (+23 → **59 FE**): `fuzzy` matcher (10), CommandPalette dispatch (5),
      ConnectionList groups + collapse + tag filter + **F43 full-order/disabled-under-filter**
      (7), F46 validation. Gates: `pnpm check`/`lint`/`test`/`build` all clean.

### M5 findings / deviations (AGENTS §9)
- **No `05`/`design-tokens` spec corrections needed.** Implemented within spec.
- **Tag filter is single-tag** (per the `activeTag: string|null` store contract), not the
  multi-tag AND/OR menu sketched in `05 §4.2`. The store is the contract; multi-tag is a
  cheap future extension if the store grows to `string[]`. Not a spec error — flagged.
- **Context-menu "Assign group ▸ / Add tag ▸"** (`05 §4.3`) are NOT nested submenus in the
  row menu; assignment is via the form's Group select + Tags editor and drag-across-groups
  (`assign_forward_group`). Avoids building nested ContextMenu submenus for M5; the
  capability is fully reachable.
- **Palette "Keyboard shortcuts (?)" entry** (`05 §15`) omitted — would need a shortcuts
  sheet; the footer hints + per-action hint labels cover discovery. Revisit if wanted.
- **Import confirm still shows mode effect, not exact N/M counts** (carried from M4) — no
  dry-run IPC; real counts reported post-import. Unchanged.
- **Anti-slop self-score: 1/10.** Dense grouped single-column list (no card grid), flat
  surfaces + hairlines, Lucide icons (no emoji), 4/8 tokens only, mono tabular numerics,
  full state machine, honest empty/loading/error, keyboard-first (⌘K + full keymap),
  reduced-motion honored, AA contrast.

### Needs a real desktop session to verify
Window resize/min-size clamp + the responsive breakpoints at 560px (container queries are
correct headless but the live webview + Tauri min bounds want eyes); ⌘K over a native
dialog; clipboard/file-picker round-trips; live `tunnel://stats` 3s cross-fade; theme
follow of OS `prefers-color-scheme`; drag-reorder + cross-group drag reassign (jsdom can't
exercise HTML5 DnD — keyboard reorder + the disabled-under-filter guard are unit-tested).

## Next action
**M5 review, then M6** (signed updater + notifications + wake polish). M5 feature-complete:
N1–N4 + N7 shipped, F43/F45–F48 fixed; 59 FE tests pass; `pnpm check`/`lint`/`test`/`build`
all clean. Interactive window/resize/DnD + real-OS-sleep wake remain for a desktop session / M6.

## Commit log (append hash + item as they land)
- `4aac549` docs: spec package v1 (pre-build baseline)
- `d146c19` chore(m0): gitignore Tauri/Node build artifacts + secrets
- `02b5082` chore(m0): frontend toolchain (pnpm + Svelte 5 + TS + Vite 6 + Tauri CLI v2)
- `376a5b4` feat(m0): scaffold Tauri v2 + Svelte shell in place at repo root
- `fe65f1d` feat(m0): Rust module tree per spec 02 §3 + real error.rs & events.rs
- `96e1f50` feat(m0): Svelte layout per spec 02 §4 + typed IPC/events/types contract
- `ce17f6b` feat(m0): register Tauri plugins as no-ops (spec 02 §8)
- `774c048` feat(m0): hidden-at-start window + minimal tray (Open/Quit)
- `794ce8f` feat(m0): tracing + tracing→log-buffer layer stub (spec 03 §18)
- `1bd1097` ci(m0): cross-platform Tauri build matrix (build-only, unsigned)
- `7e1c3a2` fix(m0): add minimal plugins.updater config so the app launches
- `e2d42e9` feat(m1): pin russh/russh-keys 0.45 + F16 spike spec corrections
- `95243b7` feat(m1): state models, stats, tunnel registry + backoff helper
- `f03910e` feat(m1): russh client, forward piping, supervisor engine + debug cmds
- `560e799` test(m1): state-machine + in-process russh integration tests
- `7fb427c` docs(m1): mark M1 complete in progress checkpoint
- `4c0cf4e` fix(m1): non-blocking RTT probe (F32) + atomic start reserve (F33) + token-driven attempt reset (F34)
- `eb0d150` test(m1): F35 coverage — keepalive-timeout, no-double-bind, conflict, park/retry
- `b27141d` docs(m1): record architecture code-review fixes (F32-F35) + host-key backlog
- `8214cdb` feat(m2): pin keyring v3 per-target backends + tempfile dev-dep
- `db190f7` feat(m2): credential store — keychain-first with plaintext fallback
- `2bba0c0` feat(m2): shared 3s stats emit sampler + registry sampler slot
- `b26e35b` feat(m2): wire sampler emit + immediate wake-reconnect + F36 guards
- `c724c00` feat(m2): sleep/wake watchdog task + setup registration
- `7564f3c` test(m2): wake sweep, sampler lifecycle, immediate wake-reconnect
- `361f4ed` feat(m2): TunnelGroup model + persisted config store (config_file.rs)
- `07665fb` feat(m2): credential store in-memory + fallback-only constructors
- `fc65ffe` feat(m2): v1->v2 migration (hardcoded per-OS probe) + lenient v1-backup parse
- `9901e4d` feat(m2): wire persisted config + credential store into AppState + boot migration
- `60a07ad` fix(m2): create fallback secrets tmp atomically at 0600 (F39)
- `2d8aa27` fix(m2): per-field serde defaults on AppSettings so partial blocks merge (NIT)
- `4a92ee2` fix(m2): route persistence through a single ordered writer (F37)
- `3f56aa1` fix(m2): run keychain password read off the async runtime (F38)
- `6610ce8` feat(m3): dynamic count tray icon + debounced state-driven menu
- `b9b9390` feat(m3): window hide-on-close, show/hide, single-instance re-show, quit
- `fdf6ab9` feat(m3): dock/taskbar visibility + autostart reconcile
- `d5b5e83` feat(m3): global bulk start_all/stop_all commands (F3)
- `331a2d4` feat(m3): wire tray/window/dock/autostart + bulk commands into setup
- `08c2e35` feat(m4): full IPC command surface + F37 error surfacing + log buffer
- `1c94b5c` chore(m4): add vitest for frontend store/helper tests
- `f4d57d6` feat(m4): forwards store reconciliation + connectedCount
- `7d86f24` feat(m4): settings/groups/logs/updater/backup store reconcilers
- `644439b` feat(m4): hydrateAll + event->store subscription wiring
- `048de6b` feat(m4): ForwardForm validation rules
- `fcd25ee` feat(m4): design token layer + global base styles
- `bf9541c` feat(m4): UI primitive library + UI plumbing helpers
- `3e205f2` feat(m4): Connections view — cards, list, form, delete confirm
- `7d69f31` feat(m4): Activity + Settings views
- `3c5aada` feat(m4): app shell + boot wiring (hydrate/subscribe/theme)
- `85939c7` test(m4): component tests + jsdom vitest setup
- `44ef253` fix(m5): emit WINDOW_FOCUS on every window show path (F44)
- `ffc4f69` feat(m5): resizable main window with spec min bounds (N2)
- `5c95728` fix(m5): F45 await subscribeEvents before hydrateAll on boot
- `a9e1876` fix(m5): F46 require at least one auth method
- `9e2d558` fix(m5): F47 distinguish password-save failure from config-save failure
- `05a8cc4` fix(m5): F48 stable log row keys instead of array index
- `6ec51d5` feat(m5): N1 command palette (⌘K) with fuzzy launcher
- `c164226` feat(m5): N3 groups/tags UI + F43 reorder-under-filter fix
- `1812a43` feat(m5): N2 responsive reflow (content-area container queries)

## M3 review outcome (focused code-review) — CLEAN
CONTINUE — 0 blockers, 0 majors; all 6 lifecycle concerns verified against code (quit teardown uses real parent-cancel+join; close=hide single-registration; single-instance plugin-first; dock truth matches v1; tray debounce trailing-edge, no dropped final state; §4 hygiene clean).
Defensive follow-ups (non-blocking — do in a pre-cutover hardening pass):
- F40 [Low] `quit_app` (window/mod.rs) lacks an overall teardown watchdog → add `select!` teardown-vs-timeout(5s)→exit(0) so a future unbounded engine await can't make the tray app unquittable.
- F41 [Nit] Quit not guarded against double-invocation → AtomicBool guard (harmless today; idempotent).
- F42 [Nit] second-instance re-show respects showInDock (v1 unconditionally un-hid taskbar) → intentional improvement, keep.
- Backlog (pre-existing): host-key verification (accept-any = v1 parity, MITM exposure) — revisit post-cutover.
