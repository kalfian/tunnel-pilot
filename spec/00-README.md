# Tunnel Pilot v2 — Rewrite Specification Package

This directory is the **single source of truth** for the Tunnel Pilot rewrite from
Flutter/Dart (v1.4.2) to **Rust + Tauri v2**. It is written to be handed to AI coding
agents so implementation is fast and correct. Read in order; each document links to the
others.

## Why the rewrite

Tunnel Pilot lives in the system tray and is idle 99% of the time, yet Flutter keeps the
Skia engine + Dart VM resident the whole time (~100–200 MB even while the window is
hidden). v2 targets **~15–30 MB idle** by using a Rust backend with the OS webview
destroyed while the window is closed.

## Locked decisions

| Area | Decision |
|---|---|
| Stack | Rust + Tauri v2 · frontend Svelte + TypeScript + Vite |
| SSH | `russh` (pure-Rust async) on `tokio` |
| Platforms | macOS, Windows, Linux — all first-class |
| Credentials | OS keychain (`keyring` crate) with plaintext fallback + UI warning |
| Persistence | JSON config in app-support dir, `schemaVersion`-tagged, atomic write |
| Updates | `tauri-plugin-updater` with a **minisign** keypair (update bundles signed + verified) — required from day one, separate from OS code-signing |
| OS code-signing | **Deferred** — v2.0 ships un-notarized (macOS) / unsigned (Windows); CI hooks written but stubbed to enable later |
| Repo | Same repo. `master` keeps Flutter until cutover; v2 on branch `rewrite/tauri`; tag `v1.4.2-flutter-final` archives the last Flutter build |
| Grouping | One exclusive `groupId` (folder, e.g. environment) + many additive `tags`; not nested |
| Window chrome | macOS = custom transparent titlebar (traffic lights); Windows/Linux = native OS decorations |
| v2.0 scope | Full v1 feature parity **+** command palette (⌘/Ctrl+K), resizable/responsive window, groups/tags, general UI/UX polish |
| Backlog v2.1+ | Import `~/.ssh/config`, non-destructive backup merge, wide detail pane + bandwidth sparkline, OS code-signing/notarization |

## Reading order

| # | Doc | What it covers |
|---|---|---|
| 01 | [01-PRD.md](01-PRD.md) | Product requirements, feature list (parity/new/backlog), success metrics, release plan, risks |
| 02 | [02-ARCHITECTURE.md](02-ARCHITECTURE.md) | Tauri process/threading model, Rust + Svelte layout, **IPC command & event catalog**, state ownership, plugins |
| 03 | [03-TECH-SPEC.md](03-TECH-SPEC.md) | Backend spec — 19 subsystems, each with behavior → Rust approach → acceptance criteria |
| 04 | [04-DATA-MODEL.md](04-DATA-MODEL.md) | Rust structs + TS types, config schema v2, keychain scheme, v1→v2 migration |
| 05 | [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md) | Design principles, IA, screens, the 3 new features, components, states, a11y, wireframes |
| — | [design-tokens.md](design-tokens.md) | Paste-ready CSS custom properties (color/type/space/motion), WCAG-verified |
| 06 | [06-MIGRATION-REPO.md](06-MIGRATION-REPO.md) | Branch/tag strategy, dir transition, CI/CD + signing, updater continuity, landing page |
| 07 | [07-ROADMAP.md](07-ROADMAP.md) | M0–M7 milestones (goal/tasks/acceptance) + v1→milestone parity checklist |
| — | [AGENTS.md](AGENTS.md) | Conventions for AI agents: dev commands, style, testing, Definition of Done, security |

## How to use this package for AI-driven development

1. Start at **07-ROADMAP.md** — pick the current milestone (M0 first).
2. For each task, the **IPC/event catalog in 02** is the contract between backend and
   frontend — implement to it, don't invent commands.
3. Backend tasks: follow **03-TECH-SPEC.md** acceptance criteria; types come from **04**.
4. UI tasks: follow **05-UI-UX-SPEC.md** + **design-tokens.md**.
5. Obey **AGENTS.md** Definition of Done before marking a task complete.

## Toolchain (already installed on this machine)

- **Rust via rustup** (`~/.cargo/bin`, 1.96+) — the official toolchain manager. Used for
  cross-compile targets (`rustup target add …`) needed for packaging. Do **not** switch to
  asdf-rust.
- **Node + pnpm via asdf** (node 25.x, pnpm 11.x).
- Still to add per platform: Tauri CLI (`pnpm add -D @tauri-apps/cli`), Linux system deps
  (WebKitGTK et al.), and per-OS Rust targets at the packaging milestone (M7). See
  [AGENTS.md](AGENTS.md) and [06-MIGRATION-REPO.md](06-MIGRATION-REPO.md).

## Reference

- Ground-truth behavior of the app being replaced lives in the Flutter source on `master`
  (`lib/`, `macos/`). Where this spec and the old `CLAUDE.md` disagree, the source and
  this spec win — the old `CLAUDE.md` is stale.
- Architecture decisions are also recorded in the vault ADR
  (`projects/tunnel-pilot/tunnel-pilot-v2-rewrite-adr.md`).
