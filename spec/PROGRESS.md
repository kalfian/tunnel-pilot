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
| M0 | Scaffold (Tauri+Svelte+CI) | 🔨 in progress | — |
| M1 | SSH core engine (russh) | ⬜ pending | — |
| M2 | Reconnect/wake/stats/persistence/keychain | ⬜ pending | — |
| M3 | Tray/window/lifecycle/autostart/dock | ⬜ pending | — |
| M4 | Full UI parity | ⬜ pending | — |
| M5 | UX improvements | ⬜ pending | — |
| M6 | Signed updater + notifications | ⬜ pending | — |
| M7 | Packaging + cutover | ⬜ pending | — |

## M0 item checklist (commit per item)
- [ ] Toolchain: add Tauri CLI (`pnpm add -D @tauri-apps/cli`), confirm rustup + node/pnpm.
- [ ] Scaffold Tauri v2 + Svelte/TS/Vite **in place at repo root** (do NOT nest a folder, do NOT touch Flutter files): `package.json`, `vite.config.ts`, `tauri.conf.json`, `src/`, `src-tauri/`.
- [ ] Rust module tree per 02 §3 (empty modules + `error.rs` AppError w/ thiserror+Serialize, `events.rs` constants for the 02 §7 catalog).
- [ ] Svelte layout per 02 §4 (empty stores/components, `lib/ipc.ts`, `lib/events.ts`, `lib/types.ts`).
- [ ] Register plugins as no-ops: autostart, notification, single-instance, updater, dialog, clipboard-manager.
- [ ] Window starts hidden; minimal tray icon with Open/Quit.
- [ ] `tracing` + tracing→log_buffer layer stub.
- [ ] CI: GitHub Actions build matrix mac/win/linux (build only, no signing) per 06 §4.
- [ ] `.gitignore`: `target/`, `node_modules/`, `dist/`, secrets, updater private key.
- [ ] Verify: `cargo build` + `pnpm build` compile; `pnpm tauri dev` launches to tray, window hidden, Open/Quit work.

## Next action
Dispatch M0 coder agent (background) to execute the M0 checklist, committing per item on `rewrite/tauri`. On completion: verify build, tick items above, update milestone table, commit this checkpoint, then start M1 (spike F16 first).

## Commit log (append hash + item as they land)
- `4aac549` docs: spec package v1 (pre-build baseline)
