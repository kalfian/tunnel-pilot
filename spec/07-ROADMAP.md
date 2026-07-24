# 07 — Roadmap: Phased Milestones for AI Agents

> Milestones sized for AI coding agents. Each: **Goal**, **Tasks**, **Acceptance
> criteria**, **Applies** (which spec sections). Milestones are mostly sequential; a few
> tasks can parallelize where files don't overlap (see [AGENTS.md](AGENTS.md) rules).
> Cross-refs: [01-PRD.md](01-PRD.md), [02-ARCHITECTURE.md](02-ARCHITECTURE.md),
> [03-TECH-SPEC.md](03-TECH-SPEC.md), [04-DATA-MODEL.md](04-DATA-MODEL.md),
> [06-MIGRATION-REPO.md](06-MIGRATION-REPO.md), [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md).

All work happens on `rewrite/tauri`. UI visual layer is owned by the design agent per
[05-UI-UX-SPEC.md](05-UI-UX-SPEC.md); coder agents wire behavior/data.

---

## M0 — Project scaffold (Tauri + Svelte + CI)

**Goal**: A running Tauri v2 + Svelte + TS + Vite app on `rewrite/tauri` with the module
skeleton, plugins registered, and CI building all three OSes.

**Tasks**
- Scaffold Tauri v2 app: `src-tauri/` (Rust) + `src/` (Svelte/TS/Vite), `package.json`,
  `vite.config.ts`, `tauri.conf.json`.
- Create the Rust module tree from [02 §3](02-ARCHITECTURE.md#3-rust-crate-layout-src-tauri)
  (empty modules + `error.rs` `AppError`, `events.rs` constants).
- Create the Svelte layout from [02 §4](02-ARCHITECTURE.md) (empty stores/components, `ipc.ts`, `events.ts`, `types.ts`).
- Register plugins (autostart, notification, single-instance, updater, dialog) as no-ops.
- Window starts hidden; a minimal tray icon with "Open"/"Quit".
- Set up `tracing` + tracing→log_buffer layer stub.
- CI: GitHub Actions build matrix (macOS/Windows/Linux) per [06 §4](06-MIGRATION-REPO.md) — build only (no signing yet).
- `.gitignore` for `target/`, `node_modules/`, `dist/`, secrets.

**Acceptance**
- [ ] `pnpm tauri dev` launches an app that sits in the tray, window hidden, no crash.
- [ ] Tray "Open" shows the window; "Quit" exits.
- [ ] CI builds succeed on all three OSes.

**Applies**: [02](02-ARCHITECTURE.md) all; [06 §4](06-MIGRATION-REPO.md); [AGENTS.md](AGENTS.md).

---

## M1 — SSH core engine (connect / forward / disconnect, no UI)

**Goal**: A correct `russh`-based tunnel engine drivable from tests (and temporary debug
commands), replicating v1's connect sequence and cleanup.

**Tasks**
- **Pin `russh`/`russh-keys` to 0.45** and, as the FIRST task, **spike the liveness design
  (F1/F7/F16)** — confirm on the pinned version: `client::Config.keepalive_interval` +
  `keepalive_max` exist; the **session future ends** when the peer misses keepalives (this is
  the connection-lost signal); the channel byte-stream API (`channel.into_stream()`). There is
  **no `ping()`/`is_closed()`**. If the API differs, update [03 Conventions/§2](03-TECH-SPEC.md)
  before proceeding. (Moved here from M2 because it is load-bearing for the whole engine.)
- `ssh/client.rs`: russh `Handler`, connect (15s timeout), auth password/identity (30s
  timeout, **identity precedence**), `keepalive_interval`/`keepalive_max` config.
- `ssh/forward.rs`: local accept loop, `direct-tcpip` channel (10s), bidirectional counting
  copy; on 3 consecutive forward failures increment the **per-attempt** `attempt_fail_count`
  and fire the **per-attempt** `attempt_fail_notify` WAKE (**never the parent**); supervisor
  re-checks authoritative per-attempt `attempt_fail_count >= 3` (F27b) → same reconnect path as
  a session drop (F26).
- `ssh/engine.rs`: single **long-lived supervisor** owning the session in-task (NO stored
  session handle, F21); **cancellation-aware bind→connect→auth** via
  `attempt_cancel.run_until_cancelled(...)` so teardown during `connecting` releases the port
  fast (F24); the **5×500ms `EADDRINUSE` bind-retry subsumes v1's 15×200ms port-release wait**
  (F25); per-attempt reset — fresh `attempt_cancel` + fresh `attempt_fail_notify` + fresh
  `attempt_fail_count` (F27a/F30); conflict handling (same-port + already-connected);
  session-future completion = connection-lost (F7).
- **Status protocol (F23/F28):** single **guarded** writer `set_status(id,new)` under the
  registry lock that **enforces the transition table (no-ops illegal transitions, e.g. drops
  `disconnecting→error`)**; supervisor writes connecting/connected/error, command handler
  writes disconnecting/disconnected; terminal `error` **parks** the supervisor (no exit).
  Pending-retry truth is the **lock-guarded `retry_requested` flag, NOT a Notify permit
  (F29)** — check-and-cleared in the same critical section as `set_status(error)` and again on
  wake; `retry_notify` is a wakeup only; `retry_forward` acts **only when status==error** (F27c)
  → set flag + fresh `attempt_cancel`, reuse supervisor, no respawn, no lost wakeup.
- `state/tunnel_registry.rs`: `TunnelHandle` with the **two-level token hierarchy** (durable
  `parent_cancel` + per-attempt `attempt_cancel = child_token()`, F6) + stable `JoinHandle` +
  **lock-guarded `retry_requested` flag** + `retry_notify` (wakeup only) + `stats_cell` +
  `StatsInner` (durable byte/conn only; per-attempt `attempt_fail_notify`/`attempt_fail_count`
  live in the supervisor, not the registry — F27a/F30); structured cancellation.
- Integration test harness against a real/local sshd (docker or CI service).

**Acceptance**: all criteria in [03 §1](03-TECH-SPEC.md#ssh), [§2 liveness](03-TECH-SPEC.md#keepalive), [§5](03-TECH-SPEC.md#concurrency), [§6](03-TECH-SPEC.md#stats).
- [ ] Traffic forwards end to end.
- [ ] Killing sshd / cutting the network ends the russh session future → tunnel goes `error`
      (F1/F7) — verified with NO app-level ping counter.
- [ ] Dead-channel (3 forward failures) tears down + reconnects when `autoReconnect=on`; never
      cancels the parent (F26).
- [ ] No stale `Notify` permit crosses an attempt boundary; a healthy reconnected session is
      never falsely torn down; forward-fail notify is a wake gated on the per-attempt
      `attempt_fail_count>=3` (F27a/b).
- [ ] A retry racing the final failure is honored (tunnel leaves `error`), never lost — retry is
      the lock-guarded `retry_requested` flag, not a permit (F29).
- [ ] Straggler failures from a dropped attempt cannot trip a teardown on the next attempt —
      the failure counter is per-attempt (F30).
- [ ] `retry_forward` fired while not parked in `error` is a no-op (status guard) (F27c).
- [ ] `set_status` enforces the table: a drop coincident with a user disconnect never yields
      `disconnecting→error` (F28) — unit-test the guarded fn over all `(current,new)` pairs.
- [ ] `status` never has two concurrent writers; retry from `error` reuses the same supervisor +
      JoinHandle (no respawn) (F23).
- [ ] Teardown during `connecting` (mid-connect/mid-auth) releases the local port within cancel
      latency, not 15–30s (F24), **and reaches `disconnected` (emits `tunnel://status`)** —
      `connecting → disconnecting → disconnected` is allowed; UI never stranded in `connecting`
      (F31).
- [ ] Cancellation leaves no leaked tasks/listeners; no double-bind; parent vs attempt token
      semantics verified.
- [ ] Byte counters accurate; latency comes from the channel-open probe.

**Applies**: [03 §§1,2,5,6](03-TECH-SPEC.md); [04 §§1,5](04-DATA-MODEL.md).

---

## M2 — Reconnect, wake, stats sampler + persistence + keychain

**Goal**: The engine self-heals; state persists; secrets live in keychain. (Core liveness
detection landed in M1 via russh keepalive + session-future signal.)

**Tasks**
- `ssh/health.rs`: single shared **3s sampler** — stats snapshot + **latency via channel-open
  probe** + emit `tunnel://stats`. **No teardown here** (liveness owned by M1's keepalive/
  session-future). Auto-start/stop ([03 §2/§6](03-TECH-SPEC.md#keepalive), F1/F4).
- `ssh/reconnect.rs`: exponential backoff (clamp 1–60), max retries, triggered by the
  session-future signal (F7), skip-on-user-disconnect via **parent-token** cancel (F6).
- `ssh/wake.rs`: monotonic-gap watchdog (>30s → sweep + immediate reconnect); **not assumed
  reliable across sleep** — session-future signal is the backstop (F15).
- `storage/config_file.rs`: atomic read-merge-write, corruption→`.corrupted`, single canonical
  `app_config_dir` (F2).
- `storage/migration.rs`: **hardcoded per-OS v1-path probe** + import (passwords→keychain);
  correct v1 bundle id/paths; Linux = no probe ([04 §12](04-DATA-MODEL.md), F2/F17).
- `credentials/mod.rs`: `keyring` with **per-target features** (F9) + `keychain_available()`
  probe + fallback file; stable `KC_SERVICE = "tunnel-pilot"`.
- Unit tests: `backoff()`, migration (real v1 config fixtures per OS), corruption, keychain
  roundtrip (mock/fallback), lenient v1-backup import (F19).

**Acceptance**: criteria in [03 §§3,4,7,8,9](03-TECH-SPEC.md); [04 §§11,12](04-DATA-MODEL.md).
- [ ] Backoff sequence exact; reconnect stops at max; user-disconnect (parent token) cancels
      even mid-backoff.
- [ ] 3s sampler emits stats + latency and **never tears down**.
- [ ] Wake sweep reconnects only dead tunnels; verified not to depend on the heuristic firing.
- [ ] Config save atomic; corrupted file quarantined; **a real v1 config at the hardcoded v1
      path migrates with no loss** on macOS & Windows; Linux is a clean fresh install.
- [ ] Passwords in keychain (features pinned) or fallback+warning, never plaintext in main config.
- [ ] v1 backup (`version:1`, no `groups`) imports leniently; legacy `sshPassword` ignored.

**Applies**: [03 §§3,4,7,8,9](03-TECH-SPEC.md); [04](04-DATA-MODEL.md) all.

---

## M3 — Tray + window + lifecycle + autostart + dock

**Goal**: Full desktop lifecycle parity — tray, hide-on-close, single-instance, dock, autostart.

**Tasks**
- `tray/icon.rs`: dynamic count icon (idle/1–9 clamp), macOS template images.
- `tray/menu.rs`: rebuild on state change (debounced), per-tunnel rows + Retry on error,
  conditional bulk Start/Stop All, update notice slot.
- `window/mod.rs`: hide-on-close intercept, `show_window`/`hide_window`, single-instance re-show.
- `platform/dock.rs`: macOS `AppHandle::set_activation_policy` (Regular/Accessory — NOT objc
  FFI, F11); Win/Linux skipTaskbar; per `showInDock`.
- `platform/autostart.rs`: sync with `launchAtLogin` on boot; start hidden.
- Implement `start_all`/`stop_all` commands (F3) and wire tray bulk Start/Stop All to them.
- Wire menu items to engine commands; emit `tunnel://status` on transitions.

**Acceptance**: criteria in [03 §§10,11,12,13,14](03-TECH-SPEC.md).
- [ ] Icon/menu reflect live state; Retry works from tray.
- [ ] Close hides (app persists); second launch re-shows; quit cleans up all tunnels.
- [ ] Dock visibility follows `showInDock`; autostart reconciled to setting.

**Applies**: [03 §§10–14](03-TECH-SPEC.md); [02 §7,§8](02-ARCHITECTURE.md).

---

## M4 — Full UI parity (connections / logs / settings / forms / backup)

**Goal**: The window reaches v1 feature parity via the IPC contract. Visuals per
[05-UI-UX-SPEC.md](05-UI-UX-SPEC.md); this milestone wires data + behavior.

**Tasks**
- Implement all IPC commands in `commands/` per [02 §6](02-ARCHITECTURE.md) and events per [02 §7](02-ARCHITECTURE.md).
- `lib/ipc.ts` typed wrappers (contract) + `lib/events.ts` subscriptions → stores.
- `app_hydrate` → full rehydrate on window show; stores mirror Rust state.
- Connections view: list (drag reorder), toggle, status badge, stat chips, per-row actions
  (edit/duplicate/delete-with-confirm/copy-ssh-command), keychain warning banner if fallback.
- ForwardForm: create/edit incl. keepalive fields; password field → `set/clear_forward_password`;
  editing a connected config force-disconnects first.
- Logs view: list (500 cap, newest-first), click-to-copy, Copy All, Clear.
- Settings view: all `AppSettings` toggles + theme; apply side effects (autostart/dock/theme).
- Backup: export (dialog path) / import (dialog + replace|merge) via `commands/backup.rs`.

**Acceptance**: parity for P1–P4, P17–P24 + [03 §§15,17,18](03-TECH-SPEC.md) (notifications/copy-ssh/logs).
- [ ] All CRUD, reorder, duplicate, delete-confirm, edit-force-disconnect behaviors match v1.
- [ ] Window fully rehydrates from Rust after being hidden/reshown.
- [ ] Backup export strips passwords; import rejects future versions; replace + merge work.
- [ ] Copy SSH command correct across bind/port/identity cases; no password.

**Applies**: [02 §§6,7](02-ARCHITECTURE.md); [03 §§15,17,18](03-TECH-SPEC.md); [04](04-DATA-MODEL.md); [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md).

---

## M5 — UX improvements (command palette, resizable, groups/tags, polish)

**Goal**: Ship the New-in-v2.0 features (N1–N4, N7).

**Tasks**
- Groups/tags backend: `commands/groups.rs` (CRUD, assign, start/stop group, list_tags);
  persist `groups` + forward `groupId`/`tags` ([04 §2,§9](04-DATA-MODEL.md)).
- Connections UI: folder headers per group with Start/Stop All, tag filter bar, ungrouped section.
- Command palette (`CommandPalette.svelte` + `stores/palette.ts`): Cmd/Ctrl+K, fuzzy search
  over tunnels + actions (connect/disconnect/edit/duplicate/delete/jump-tab/bulk).
- Resizable window: enable resize + min size in `tauri.conf.json`; responsive reflow
  (design agent). Persist last window size optionally.
- Non-destructive import merge option (N7) surfaced in backup UI.
- General polish pass with design agent.

**NOT in this milestone / NOT in v2.0 (deferred to v2.1 — see [01 §3.3](01-PRD.md)):**
- **Wide detail pane** — no dedicated per-tunnel detail panel. v2.0 shows **list + stat
  chips only**. Do not build it here.
- **Bandwidth sparkline** — no per-tunnel mini-chart / history sampling. Deferred.
  (Design agent excludes the same in [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md).)

**Acceptance**: N1–N4, N7 from [01 §3.2](01-PRD.md).
- [ ] Palette connects/disconnects/edits a tunnel purely by keyboard; fuzzy match works.
- [ ] Groups: assign, filter by tag, Start/Stop All per group.
- [ ] Window resizes with sane min size; layout reflows.
- [ ] Tunnel presentation is list + stat chips only (no wide detail pane, no sparkline).

**Applies**: [01 §3.2](01-PRD.md); [02 §6.2](02-ARCHITECTURE.md); [04 §§2,9,11](04-DATA-MODEL.md); [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md).

---

## M6 — Updater (signed bundles) + notifications + wake polish

**Goal**: Self-update with **minisign-signed update bundles** and unified notifications fully
wired to UI + tray. (This is updater *bundle* signing — free, self-generated key — NOT OS
code-signing, which is skipped; see [03 §16](03-TECH-SPEC.md#updater), [06 §4](06-MIGRATION-REPO.md).)

**Tasks**
- `updater/mod.rs` + `commands/updater.rs`: check/install/skip; `update://status` + `update://progress`.
- Wire tray update notice + in-window update prompt + progress indicator.
- `platform/notify.rs`: connect/disconnect/error (unexpected only) + update-once-per-version;
  macOS permission timing.
- **F5 SPIKE (do early in M6): verify `tauri-plugin-notification` on an UNSIGNED macOS build.**
  macOS `UNUserNotificationCenter` may refuse to display without a signed bundle. If it fails,
  document the known limitation and confirm the tray-state + in-window-log fallback; **M6
  acceptance must NOT assume macOS notifications work** ([03 §15](03-TECH-SPEC.md#notifications)).
- Verify wake detection + reconnect under **real system sleep on each OS** (F15) — document
  any OS where the gap heuristic doesn't fire (session-future signal still recovers).
- Generate the **minisign** updater keypair; embed pubkey in `tauri.conf.json`; CI bundle
  signing wired ([06 §4](06-MIGRATION-REPO.md)). Private key → CI secret only.

**Acceptance**: criteria in [03 §§15,16](03-TECH-SPEC.md); [01 N6](01-PRD.md).
- [ ] A minisign-signed v2→v2 update installs; tampered/unsigned bundle rejected.
- [ ] Updater signing works independently of OS code-signing being off.
- [ ] Notification behavior on unsigned macOS is **verified and documented** (works, or known
      limitation + fallback confirmed) — not assumed.
- [ ] Notifications fire per rules; silent on user disconnect; update notice once per version.

**Applies**: [03 §§15,16,4](03-TECH-SPEC.md); [06 §§4,6](06-MIGRATION-REPO.md).

---

## M7 — Packaging / release + cutover

**Goal**: Ship v2.0.0 and cut over to `master`. **OS code-signing/notarization is NOT part
of v2.0** (open-source + unfunded — see [01 §3.3](01-PRD.md), [06 §4](06-MIGRATION-REPO.md));
this milestone ships unsigned installers with documented workarounds.

**Tasks**
- Finalize `tauri.conf.json` bundling (macOS universal **un-notarized**, Windows installer
  **unsigned**, Linux AppImage/deb/rpm); macOS `LSUIElement`, unsandboxed entitlements, custom
  transparent titlebar on macOS / native decorations on Win+Linux ([03 §§14,platform](03-TECH-SPEC.md#platform)).
- CI release workflow: **updater bundle minisign signing** (required) + `latest.json`; OS
  code-signing env left stubbed/commented ([06 §4](06-MIGRATION-REPO.md)).
- Publish install workarounds (Gatekeeper right-click→Open; SmartScreen More info→Run anyway)
  to README + landing page ([06 §§7,8](06-MIGRATION-REPO.md)).
- **Measure RAM per the resolved methodology**: RSS of app process(es) with window **HIDDEN
  after 60s idle**, tray active, on each OS (webview torn down when hidden); also cold-start
  ([01 §5](01-PRD.md)).
- v1 bridge release + landing page update ([06 §§6,7](06-MIGRATION-REPO.md)).
- Cutover checklist ([06 §3](06-MIGRATION-REPO.md)); move Flutter → `legacy/flutter/` (KEEP,
  do not delete); bump to 2.0.0; merge `rewrite/tauri` → `master` (push/release via CI only).

**Acceptance**
- [ ] Installers for all three OSes; **minisign-signed** self-update verified end to end.
- [ ] Idle RAM ≤ 30 MB measured with window hidden after 60s idle on each OS; result documented.
- [ ] Install workarounds documented in README + landing page.
- [ ] v1→v2 migration verified on real config; bridge notice live.
- [ ] `master` releases v2.0.0 from CI.

**Applies**: [01 §5,§6](01-PRD.md); [03 §§16,14,platform](03-TECH-SPEC.md); [06](06-MIGRATION-REPO.md) all.

---

## Parity checklist (v1 feature → milestone)

| v1 feature (from [01 §3.1](01-PRD.md)) | Milestone |
|---|---|
| P1 CRUD, duplicate, delete-confirm, edit-force-disconnect | M4 (backend M1/M2) |
| P2 drag reorder (array order) | M4 |
| P3 5-state toggle (ignore disconnecting / retry on error) | M1 (engine) + M4 (UI) |
| P4 port-release on rebind (folded into 5×500ms bind-retry, F25) | M1 |
| P5 auto-reconnect backoff | M2 |
| P6 russh keepalive liveness (session-future signal) | **M1** (F1/F7) |
| P7 3s stats sampler (NOT teardown) | M2 (sampler) — liveness itself is M1 |
| P8 dead-channel detection | M1 |
| P9 wake-from-sleep | M2 (M6 real-OS verify, F15) |
| P10 race guard → two-level tokio cancellation | M1 (F6) |
| P11 live per-tunnel stats (single 3s cadence) | M1 (calc) + M2 (sampler) + M4 (chips) |
| P12 dynamic tray icon 1–9 | M3 |
| P13 tray menu rebuild + retry + bulk + update notice | M3 (update notice M6) |
| global bulk `start_all`/`stop_all` (v1 connectAll/disconnectAll, F3) | M3 (commands+tray) / M5 (palette) |
| P14 notifications (unexpected only) | M6 |
| P15 launch at startup | M3 |
| P16 dock/taskbar visibility | M3 |
| P17 hide-on-close | M3 |
| P18 single instance | M3 |
| P19 theme light/dark/system | M4 |
| P20 logs viewer (500, copy, clear) | M4 |
| P21 backup export/import (replace) | M4 |
| P22 self-update (→ signed) | M6 |
| P23 copy SSH command | M4 |
| P24 persistence (JSON, corruption) | M2 |
| P25 keychain + fallback | M2 |
| **N1** command palette | M5 |
| **N2** resizable/responsive | M5 |
| **N3** groups/tags + bulk | M5 |
| **N4** UI/UX polish | M5 |
| **N5** keychain-first creds | M2 |
| **N6** signed updater | M6 |
| **N7** non-destructive merge | M5 |
| v1→v2 migration | M2 |
| Cutover + packaging | M7 |
