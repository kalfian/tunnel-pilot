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
| M2 | Reconnect/wake/stats/persistence/keychain | ⬜ pending | — |
| M3 | Tray/window/lifecycle/autostart/dock | ⬜ pending | — |
| M4 | Full UI parity | ⬜ pending | — |
| M5 | UX improvements | ⬜ pending | — |
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

## Next action
Start **M2 — Reconnect/wake/stats sampler + persistence + keychain**. Backoff + supervisor reconnect loop + is_closed liveness already landed in M1; M2 adds: `ssh/health.rs` shared 3s emit sampler (reads stats cells, no teardown), `ssh/wake.rs` monotonic-gap watchdog (pokes the existing `wake_notify` arm), `storage/config_file.rs` (atomic read-merge-write + corruption→`.corrupted`), `storage/migration.rs` (hardcoded per-OS v1 probe), `credentials/mod.rs` (`keyring` per-target features + fallback). Swap AppState's in-memory `configs`/`passwords` for the persisted file + keychain.

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
