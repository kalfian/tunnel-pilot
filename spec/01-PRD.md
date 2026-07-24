# 01 — Product Requirements: Tunnel Pilot v2

> Rewrite of Tunnel Pilot (Flutter/Dart v1.4.2) to **Rust + Tauri v2 + Svelte + TypeScript + Vite**.
> This document defines what v2.0 must do and how we measure success.
> Cross-refs: [02-ARCHITECTURE.md](02-ARCHITECTURE.md), [03-TECH-SPEC.md](03-TECH-SPEC.md),
> [04-DATA-MODEL.md](04-DATA-MODEL.md), [06-MIGRATION-REPO.md](06-MIGRATION-REPO.md),
> [07-ROADMAP.md](07-ROADMAP.md), [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md) (owned by design agent).

## 1. Vision & Problem Statement

Tunnel Pilot is a cross-platform (macOS / Windows / Linux) SSH local port-forwarding
manager that lives in the system tray — no Dock/taskbar presence by default. It lets a
developer define named `ssh -L` forwards, toggle them on/off, and keep them alive across
flaky networks and sleep/wake cycles.

**The problem driving the rewrite is RAM.** Tunnel Pilot is a tray-idle utility: for
99% of its lifetime the window is hidden and the app is just watching a handful of
tunnels. Flutter's embedder holds a full Skia + Dart VM resident even when nothing is
drawn, costing **~100–200 MB RSS idle**. That is unacceptable for a background utility
users expect to "just sit there." A Tauri app renders through the OS webview (only spun
up when the window is shown) and does its real work in a lean Rust/tokio core, targeting
**~15–30 MB idle**.

Secondary drivers:
- **Unsigned self-update** in v1 (raw GitHub HttpClient + inline install scripts, no
  signature verification) — a security liability. v2 adopts `tauri-plugin-updater` with
  signed artifacts.
- **Fixed 700×600 window** and no keyboard-driven workflow — v2 adds a resizable
  responsive layout and a command palette.
- **No organization** for users running many tunnels across environments — v2 adds
  groups/tags with bulk Start/Stop.

## 2. Target Users

- Backend / infra / platform engineers who tunnel to databases, admin panels, internal
  services behind a bastion.
- Power users running **many** forwards across multiple environments (staging / prod /
  per-client) who need organization and bulk control.
- Users on unreliable networks / VPNs who need aggressive liveness detection and
  auto-reconnect.
- Cross-platform teams: identical behavior on macOS, Windows, Linux.

## 3. Feature List

### 3.1 Parity (must match v1 behavior exactly — see [03-TECH-SPEC.md](03-TECH-SPEC.md))

| # | Feature | Notes |
|---|---------|-------|
| P1 | Forward config CRUD | Create/read/update/delete; duplicate appends " (copy)"; delete needs confirm; editing a *connected* config force-disconnects first. |
| P2 | Reorder via drag | List order persisted as array position (no explicit order field). |
| P3 | Connect/disconnect toggle | 5-state machine: disconnected/connecting/connected/disconnecting/error. Click during `disconnecting` ignored; click during `error` = **retry**. |
| P4 | Port-release on rebind | Dodge TIME_WAIT/late local-port release. v2 folds v1's separate 15×200ms `_waitForPortAvailable` poll into the **5×500ms `EADDRINUSE` bind-retry** (F25) — one mechanism, same effect. See [03 §1](03-TECH-SPEC.md#ssh). |
| P5 | Auto-reconnect | Exponential backoff `delaySec * 2^attempts` clamped 1–60s; stop after `autoReconnectMaxRetries`; skip if user-disconnected or config removed. |
| P6 | Keep-alive / liveness | **russh `keepalive_interval` + `keepalive_max` per-config** (interval default 30→10 if 0; max default 5→3 if 0). This is the teardown authority — when the peer misses `keepalive_max` keepalives the **session future ends** = connection lost. russh has **no `ping()`/`is_closed()`** (F1). See [03 §2](03-TECH-SPEC.md#keepalive). |
| P7 | Health sampler (NOT teardown) | Single shared **3s sampler** that only samples stats + measures latency via a channel-open probe + reflects the supervisor liveness flag. **It does NOT count ping failures or tear down** — liveness is owned by P6 (F1). |
| P8 | Dead-channel detection | Tear down tunnel after 3 consecutive forward-*channel* failures (distinct from P6 session-level liveness). |
| P9 | Wake-from-sleep detection | On resume after >30s inactivity, probe connected tunnels, force-reconnect dead ones. Heuristic is best-effort (not guaranteed across OS sleep, F15); P6's session-future signal is the backstop. |
| P10 | Race/generation guard | v1 used a per-config generation counter; v2 uses a **two-level tokio cancellation hierarchy** (durable parent token + per-attempt child tokens, F6) — see [03 §5](03-TECH-SPEC.md#concurrency). |
| P11 | Live per-tunnel stats | Active conn count, cumulative bytes up/down, last latency (via channel-open probe), uptime; **single 3s cadence** (F4). |
| P12 | Dynamic tray icon | Grey idle; blue badge with connected count 1–9 (clamped at 9). macOS template images. |
| P13 | Tray menu | Rebuilds on state change; per-tunnel rows with status + Retry on error; conditional bulk Start/Stop All; update-available notice at top. |
| P14 | Notifications | On connect/disconnect/error for *unexpected* states only (user-initiated disconnects are silent); update-available once per version. |
| P15 | Launch at startup | `tauri-plugin-autostart`; re-synced with OS on every launch. |
| P16 | Dock/taskbar visibility | `showInDock` setting; window open → show in dock iff `showInDock`; window close → always hide. |
| P17 | Window hide-on-close | Close hides window, app stays alive in tray. |
| P18 | Single instance | `tauri-plugin-single-instance`; second launch re-shows window. |
| P19 | Theme light/dark/system | Custom design tokens; no Material ripple. |
| P20 | Logs viewer | In-memory only (not persisted), cap 500 newest-first, click row / Copy All to clipboard, Clear. |
| P21 | Backup export/import | Export strips passwords; versioned file; import rejects version > current; validates then **replaces** whole list. |
| P22 | Self-update | Replaced by signed `tauri-plugin-updater` (see [P-updater in 03](03-TECH-SPEC.md#updater)). |
| P23 | Copy SSH command | Build `ssh -N -L ...` string exactly like v1: **always emit `-p <port>`** (not conditional), bind prefix only if not 127.0.0.1, identity path quoted only if it has a space. See [03 §17](03-TECH-SPEC.md#copy-ssh). |
| P24 | Persistence | Single JSON `tunnel_pilot_config.json` in app-support dir; corruption → copy `.corrupted`, start fresh. |
| P25 | Credential handling | OS keychain with plaintext fallback (see below + [04](04-DATA-MODEL.md)). |

### 3.2 New in v2.0

| # | Feature | Description |
|---|---------|-------------|
| N1 | **Command palette** | Cmd/Ctrl+K opens fuzzy-search palette; connect/disconnect/edit/duplicate/delete a tunnel by keyboard; jump to tabs; run bulk actions. |
| N2 | **Resizable + responsive window** | Window resizable (min size enforced); layout reflows for narrow/wide. (v1 was fixed 700×600.) |
| N3 | **Groups / tags** | Each forward has an optional `groupId` and/or `tags`. Group by folder or filter by tag; **Start/Stop All per group**; filter the list by tag. |
| N4 | **UI/UX polish** | General refinement, owned by design agent — see [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md). |
| N5 | **Keychain-first credentials** | Passwords stored in OS keychain (`keyring` crate); plaintext fallback only when keychain unavailable, with a **visible UI warning**. Config stores a `hasStoredPassword` flag, not the secret. |
| N6 | **Signed auto-update (updater bundles)** | Update bundles are signed with a minisign key and verified against the pubkey embedded in the app via `tauri-plugin-updater`. This is the **updater** signature — independent of OS code-signing/notarization, which is deferred (see §4 & [06](06-MIGRATION-REPO.md)). |
| N7 | **Non-destructive backup merge (optional)** | Import offers merge-or-replace; replace remains the default. |

### 3.3 Backlog (v2.1+)

- **Wide detail pane** — a dedicated per-tunnel detail panel in a wide layout (v2.0 ships list + stat chips only). Deferred; design agent tracks the same in [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md).
- **Bandwidth sparkline** — per-tunnel bandwidth mini-chart in the row/detail (needs history sampling). Deferred (see also bandwidth graphs below).
- Import from `~/.ssh/config` (parse Host blocks → forward templates).
- SSH agent / `ssh-agent` auth.
- Remote (`-R`) and dynamic (`-D` / SOCKS) forwarding.
- Per-tunnel bandwidth graphs / history persistence.
- OS code-signing + notarization (macOS Developer ID + notarize, Windows Authenticode) — **optional future enablement, not a planned milestone.** v2.0 ships without OS code-signing/notarization because the project is open-source and unfunded and signing certs cost money we don't have (Apple Developer ID ~$99/yr; Windows OV/EV cert ~$100s/yr). CI hooks are written but stubbed so a future maintainer (or if the project gets funded/donations) can turn signing on without restructuring. See [06 §4](06-MIGRATION-REPO.md).
- Config sync across machines.

## 4. Non-Goals (v2.0)

- No `~/.ssh/config` import (moved to backlog).
- No remote/dynamic forwarding — local (`-L`) only.
- No telemetry / analytics.
- No cloud account or sync.
- No mobile targets.
- No sandboxed macOS build — the app stays unsandboxed (needs arbitrary SSH,
  reading `~/.ssh/id_*`, self-update writes). See platform notes in [03](03-TECH-SPEC.md#platform).

## 5. Success Metrics

| Metric | Target | Baseline (Flutter v1) |
|--------|--------|-----------------------|
| **Idle RAM** (window hidden, tray only, 0 tunnels) | **15–30 MB RSS** | ~100–200 MB |
| **Active RAM** (window hidden, 3 tunnels connected) | ≤ 60 MB RSS | ~150–250 MB |
| **Cold-start to tray-ready** | ≤ 1.5 s | n/a (measure v1 for comparison) |
| **Window-show latency** (first paint after tray click) | ≤ 400 ms | n/a |
| **Feature parity** | 100% of P1–P25 pass acceptance criteria in [03](03-TECH-SPEC.md) | — |
| **Binary size** (per-platform installer) | Documented; expect << Flutter | — |
| **Updater signature verification** | Enforced — update bundles minisign-verified against embedded pubkey (unsigned/tampered rejected). NOTE: OS code-signing/notarization is deferred (§4). | Absent |

**RAM measurement methodology (resolved):** measure the RSS of the app process(es) with the
window **HIDDEN** after **60s idle**, tray active, on each OS (macOS/Windows/Linux). The
webview is torn down when the window is hidden, so this captures the true tray-idle
footprint. Applied as the M7 acceptance gate ([07](07-ROADMAP.md)).

## 6. Release Plan

- Develop on branch `rewrite/tauri`; Flutter stays on `master` until cutover.
  Tag `v1.4.2-flutter-final` archives the last Flutter build. See [06](06-MIGRATION-REPO.md).
- **v2.0.0** ships when P1–P25 parity + N1–N6 pass. Cutover merges `rewrite/tauri` → `master`.
- Existing v1 users cannot auto-migrate (incompatible updater/packaging). Ship a final v1
  release and/or in-app notice pointing to the v2 download. See [06 §updater continuity](06-MIGRATION-REPO.md).
- Post-cutover, v2.1 picks up backlog items.

## 7. Risks

| Risk | Impact | Mitigation |
|------|--------|-----------|
| **`russh` has no `ping()`/`is_closed()`** (dartssh2-only) | v1's liveness model doesn't port; wrong design = tunnels never detect drops | Liveness = russh `keepalive_interval`+`keepalive_max` → session future ends = connection lost (F1/F7); 3s timer is stats/latency-only. **Spike this FIRST in M1** on pinned russh 0.45 ([03 §2](03-TECH-SPEC.md#keepalive)). |
| **v1 config path mismatch on upgrade** (macOS bundle id `com.kalfian.tunnelpilot` no underscore; Windows `%APPDATA%\kalfian\Tunnel Pilot\`) | Tauri dir resolution won't find v1 config → silent total data loss on upgrade | Hardcoded per-OS v1-path probe in migration, independent of Tauri dirs (F2, [04 §12](04-DATA-MODEL.md)); config lives outside the app bundle so it survives. |
| `keyring` v3 ships no backend by default | `keychain_available()` always false → every password silently plaintext | Pin per-target features: `apple-native`/`windows-native`/`sync-secret-service`+`crypto-rust` (F9, [03 §9](03-TECH-SPEC.md#credentials)). Linux headless still falls back (handled). |
| Keychain unavailable on Linux headless / CI | Password loss or crash | Plaintext fallback + visible warning; feature-detect at runtime (see [03 §credentials](03-TECH-SPEC.md#credentials)). |
| Updater minisign key management | Broken/unsafe updates | Generate + securely store the Tauri updater minisign keypair; CI signs bundles; public key embedded in `tauri.conf.json`. See [06](06-MIGRATION-REPO.md). |
| **Unsigned macOS build may break notifications (F5)** | `UNUserNotificationCenter` often needs a signed bundle → notifications silently fail on macOS | M6 spike verifies on the unsigned build; if broken, document as known limitation + fall back to tray state / in-window log. M6 acceptance does NOT assume they work ([03 §15](03-TECH-SPEC.md#notifications)). |
| **Unsigned OS distribution (deliberate baseline)** | Install-time friction: macOS Gatekeeper blocks un-notarized apps; Windows SmartScreen warns | v2.0 ships without OS code-signing because the project is open-source and unfunded (signing certs cost money we don't have) — a deliberate baseline, not a temporary gap. Permanent user-facing workarounds documented for README/landing: **macOS** right-click → Open (bypass Gatekeeper); **Windows** "More info → Run anyway" (bypass SmartScreen). CI signing hooks stubbed for optional future enablement if funded. See [06 §4](06-MIGRATION-REPO.md). Note: **update-bundle signing is free (self-generated minisign key) and IS enforced** — only OS code-signing (needs a paid cert) is skipped. |
| macOS notification permission timing | Missed notifications | Request permission at an explicit, well-timed moment (not startup race) — see [03 §notifications](03-TECH-SPEC.md#notifications). |
| v1 users stranded on incompatible updater | Fragmented user base | Bridge release + landing-page + in-app notice (see [06](06-MIGRATION-REPO.md)). |
| Tray behavior divergence across OS (menu rebuild, template icons) | UX inconsistency | Per-OS acceptance criteria in [03 §tray](03-TECH-SPEC.md#tray) and parity checklist in [07](07-ROADMAP.md). |
