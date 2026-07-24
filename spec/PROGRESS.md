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
| M1 | SSH core engine (russh) | ⬜ pending | — |
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

## Next action
Start **M1 — SSH core engine**. FIRST task: spike F16 — pin `russh`/`russh-keys` to 0.45 and confirm the API (`Config.keepalive_interval`/`keepalive_max`, session-future-end = connection-lost, `channel.into_stream()`; no `ping()`/`is_closed()`). Update 03 if the API differs before building the engine.

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
