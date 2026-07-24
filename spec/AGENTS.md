# AGENTS.md — Conventions for AI Agents in Tunnel Pilot v2

> Rules of the road for AI coding agents working the Tauri v2 rewrite on `rewrite/tauri`.
> Read the relevant spec section before touching a subsystem.
> Cross-refs: [01-PRD.md](01-PRD.md), [02-ARCHITECTURE.md](02-ARCHITECTURE.md),
> [03-TECH-SPEC.md](03-TECH-SPEC.md), [04-DATA-MODEL.md](04-DATA-MODEL.md),
> [06-MIGRATION-REPO.md](06-MIGRATION-REPO.md), [07-ROADMAP.md](07-ROADMAP.md),
> [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md).

## 1. The IPC contract is the source of truth

- Every backend capability is a `#[tauri::command]` listed in [02 §6](02-ARCHITECTURE.md)
  with a matching typed wrapper in `src/lib/ipc.ts`. **Do not** call `invoke()` with raw
  strings from components — always go through `lib/ipc.ts`.
- Rust models and `src/lib/types.ts` must stay in lockstep (see [04](04-DATA-MODEL.md)).
  All serde structs use `#[serde(rename_all = "camelCase")]`; TS types are camelCase.
- Adding/changing a command = update, in one change: the Rust command, the `invoke_handler`
  registration, `lib/ipc.ts`, `lib/types.ts`, and the tables in [02](02-ARCHITECTURE.md).
- Events (Rust→FE) are constants in `src-tauri/src/events.rs`, subscribed in `lib/events.ts`.
  Never emit an ad-hoc event name not in the [02 §7](02-ARCHITECTURE.md) catalog. Use the
  exact event strings (`tunnel://status`, `tunnel://stats`, `log://line`, `log://cleared`,
  `update://status`, `update://progress`, `window://focus`, `forwards://changed`,
  `groups://changed`, `settings://changed`).
- **Global bulk vs per-group bulk are distinct commands** — `start_all`/`stop_all`
  (global; tray + palette + keymap) live in `commands/forwards.rs`; `start_group`/`stop_group`
  (per folder) live in `commands/groups.rs`. Do not implement one by looping the other in the
  frontend — call the dedicated command.

## 2. Dev commands

```bash
# Frontend deps (pnpm preferred; npm acceptable — pick one and stay consistent)
pnpm install

# Run the app in dev (Rust + Vite HMR)
pnpm tauri dev

# Build a release bundle for the current OS
pnpm tauri build

# Rust: from src-tauri/
cargo build
cargo test                 # unit + integration tests
cargo clippy --all-targets -- -D warnings
cargo fmt

# Frontend
pnpm check                 # svelte-check / tsc
pnpm test                  # component/unit tests (vitest)
pnpm lint                  # eslint + prettier
```

Do **not** `git push` from the CLI — pushing/releasing goes through CI (project rule, see
[06 §1](06-MIGRATION-REPO.md)). Commit locally; releases are tag-triggered in Actions.

## 3. Directory conventions

- Rust lives in `src-tauri/src/` with the module tree in [02 §3](02-ARCHITECTURE.md). One
  subsystem per module dir; keep `commands/` thin (parse args → call a service in
  `ssh/`/`storage/`/`credentials/`/etc.).
- Svelte lives in `src/` per [02 §4](02-ARCHITECTURE.md). Components are presentational;
  state flows through stores fed by events.
- During dev, Flutter v1 remains in `lib/`, `macos/`, etc. — **do not modify Flutter code**
  on `rewrite/tauri`. It moves to `legacy/flutter/` at cutover ([06 §2](06-MIGRATION-REPO.md)).
- Never commit secrets, `target/`, `node_modules/`, `dist/`, or the updater private key
  (org policy + [06](06-MIGRATION-REPO.md)).

## 4. Rust style

- **Errors**: `thiserror` for the library-facing `AppError` enum in `error.rs`; it derives
  `Serialize` so it crosses IPC. Use `anyhow` only at binary edges (setup/main), never in a
  public command return type. Command return type is always `Result<T, AppError>`.
- **No `unwrap()`/`expect()`** in non-test code except for provably-infallible invariants
  with a comment justifying it. Prefer `?` and typed errors.
- **Async**: tokio only. Never block the runtime — no `std::thread::sleep`, no sync file I/O
  on the async path (use `tokio::fs`). Never hold a `Mutex`/`RwLock` guard across an `.await`
  that does network I/O ([03 §5](03-TECH-SPEC.md#concurrency)).
- **Cancellation**: use `CancellationToken` + `select!` for all long-lived tasks; disconnect
  = cancel + await join. No generation counters.
- **Logging**: `tracing` (`info!`/`warn!`/`error!` with structured fields). User-visible
  events also append a `LogEntry` to the buffer ([03 §18](03-TECH-SPEC.md#logs)). Never log
  passwords/secrets (org policy — use placeholders if you must reference them).
- `cargo fmt` + `cargo clippy -D warnings` must pass.

## 5. Svelte / TypeScript style

- TypeScript strict mode. No `any` — model everything with the types in `lib/types.ts`.
- Components are dumb: props in, events/store-writes out. No `invoke()` inside a component —
  call `lib/ipc.ts`.
- Stores are the only place that reconcile event payloads into UI state ([02 §5](02-ARCHITECTURE.md)).
- On window show/boot, call `app_hydrate()` and hydrate all stores before rendering live
  data — never assume the frontend kept state while the window was hidden.
- Visual details (tokens, spacing, monospace stack, states) are owned by the design agent —
  see [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md). Coder agents wire behavior, not aesthetics.
- `prettier` + `eslint` + `svelte-check` must pass.

## 6. Testing strategy

**Rust (priority — this is where correctness lives):**
- Unit tests, pure functions: `backoff()` clamping ([03 §3](03-TECH-SPEC.md#reconnect)),
  `copy_ssh_command` string builder ([03 §17](03-TECH-SPEC.md#copy-ssh)), effective-keepalive
  normalization (0→10 / 0→3), tray icon selection (count→asset, clamp 9).
- State-machine tests: connect→connecting→connected→disconnecting→disconnected transitions;
  ignore-click-during-disconnecting; retry-on-error; **two-level token semantics** (parent
  cancel kills reconnect mid-backoff; attempt token swaps don't disturb parent) ([03 §5](03-TECH-SPEC.md#concurrency), F6).
- Migration tests: feed a **real v1 `tunnel_pilot_config.json` fixture per OS** and assert the
  hardcoded v1-path probe finds it, v2 output is correct, password→keychain (mock/fallback),
  Linux = no probe ([04 §12](04-DATA-MODEL.md), F2/F17). Plus lenient v1-backup import
  (`version:1`, no `groups`, legacy `sshPassword` ignored — F19).
- Persistence tests: atomic write, corruption→`.corrupted`, read-merge-write preserves siblings.
- Integration: SSH engine against a local sshd (docker/CI service) — end-to-end forward,
  byte counters, cancellation leaves no leaks, and **connection-lost via session-future ending**
  (kill sshd → status `error`); assert there is **no app-level ping-failure counter** (F1/F7).

**Frontend:**
- Vitest component tests for ConnectionRow, ForwardForm validation, CommandPalette fuzzy
  match, backup import mode selection.
- Mock the IPC layer (`lib/ipc.ts`) so components test against the contract, not a live backend.

## 7. Definition of Done (per task)

A task is done when:
- [ ] Behavior matches the referenced spec section's **Acceptance criteria**.
- [ ] Tests added/updated (Rust unit/integration and/or frontend) and passing.
- [ ] `cargo fmt`, `cargo clippy -D warnings`, `pnpm check`, `pnpm lint` all clean.
- [ ] If the IPC surface changed: Rust command + handler + `lib/ipc.ts` + `lib/types.ts` +
      [02](02-ARCHITECTURE.md) tables all updated together.
- [ ] No secrets/`unwrap()`-in-prod/blocking-on-async introduced.
- [ ] Cross-platform concerns considered (macOS/Windows/Linux) or explicitly deferred with a note.
- [ ] Relevant spec doc updated if the implementation revealed a needed correction.

## 8. Security rules (non-negotiable)

- **No secrets in config when keychain is available.** Passwords go to the OS keychain via
  `credentials/`; the main config JSON stores only `hasStoredPassword`
  ([03 §9](03-TECH-SPEC.md#credentials), [04 §10](04-DATA-MODEL.md)).
- **Plaintext fallback only when keychain is unavailable**, in a separate secrets file, with
  a visible UI warning; never in the main config or backups.
- **Never** log, emit over IPC, include in `copy_ssh_command`, or write to backups any
  password (org policy: no credentials in output — use placeholders when referencing).
- **Signed updates only**: `tauri-plugin-updater` with the embedded public key; the private
  key lives solely in CI secrets ([03 §16](03-TECH-SPEC.md#updater), [06 §4](06-MIGRATION-REPO.md)).
  Never weaken signature verification.
- Keep macOS **unsandboxed** deliberately ([03 §platform](03-TECH-SPEC.md#platform)); do not
  add App Sandbox without a spec change.
- Tauri v2 capabilities/ACL: expose only the commands in [02 §6](02-ARCHITECTURE.md) to the
  window; scope fs/dialog narrowly.

## 9. When something in the spec is wrong or ambiguous

Specs are the contract but not infallible. If implementation reveals a spec error or an
undecided edge case (e.g. a `russh` API that doesn't match the assumed shape), **stop and
flag it** rather than silently diverging: note the discrepancy, propose the fix, and update
the affected spec doc as part of the change so the contract stays true.
