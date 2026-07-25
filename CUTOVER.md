# Tunnel Pilot v2.0.0 — Cutover Checklist

> The remaining steps to ship **v2.0.0** (Rust + Tauri) and cut over from the Flutter v1
> `master`. Each item is marked **DONE** (landed on `rewrite/tauri`) or **PENDING** with who
> must do it: **manual**, **CI**, or **device** (needs a real desktop / display / OS).
>
> Ground rules (do NOT violate): commits are local; **push/release happens ONLY via CI**
> (`git push` from the CLI is forbidden — project rule / [06 §1](spec/06-MIGRATION-REPO.md)).
> Do **not** delete Flutter — it moves to `legacy/flutter/`. No secrets in any tracked file.
>
> Spec cross-refs: [06 §3 cutover](spec/06-MIGRATION-REPO.md), [06 §4 CI/signing](spec/06-MIGRATION-REPO.md),
> [06 §6 upgrade path](spec/06-MIGRATION-REPO.md), [01 §5 RAM](spec/01-PRD.md), [07 M7](spec/07-ROADMAP.md).

---

## 0. What is already DONE (on `rewrite/tauri`)

- ✅ **Bundling finalized** (`src-tauri/tauri.conf.json`): per-OS targets (macOS `app`+`dmg`,
  Windows `nsis`, Linux `appimage`+`deb`+`rpm`), un-notarized macOS + `LSUIElement`
  (`src-tauri/Info.plist`) + unsandboxed `entitlements.plist` (v1 parity), NSIS unsigned,
  publisher/copyright/category/identifier (`com.kalfian.tunnelpilot`), icons, macOS overlay
  titlebar / Win+Linux native decorations. OS code-signing deliberately absent.
- ✅ **Updater artifacts wired**: `createUpdaterArtifacts: true`, endpoint =
  `github.com/kalfian/tunnel-pilot/releases/latest/download/latest.json`, minisign **pubkey
  committed** in `tauri.conf.json` (verified to match the keypair). Self-updater backend +
  FE banner + tray notice landed in M6.
- ✅ **Version 2.0.0** consistent across `tauri.conf.json`, `src-tauri/Cargo.toml`, `package.json`.
- ✅ **CI release workflow** (`.github/workflows/tauri-release.yml`): tag-triggered (`v2.*`),
  cross-platform build + **minisign updater-bundle signing** + `latest.json`; OS code-signing
  env left commented for future funded enablement.
- ✅ **README.md** + **docs/index.html**: install workarounds, v1→v2 upgrade path, signed-update
  note, Rust/Tauri build-from-source.
- ✅ Tests green on the branch: 113 Rust + 72 FE (all gates: `cargo fmt`/`clippy -D warnings`/
  `pnpm check`/`lint`/`test`/`build`).

---

## 1. CI secrets to set (PENDING — manual, one-time)

Set these as **GitHub repository secrets** (Settings → Secrets and variables → Actions).
The `tauri-release.yml` workflow reads them for minisign updater-bundle signing:

| Secret | Value source |
|--------|--------------|
| `TAURI_UPDATER_PRIVATE_KEY` | Contents of `~/tunnel-pilot-updater-keys/tunnel-pilot-updater.key` |
| `TAURI_UPDATER_KEY_PASSWORD` | The password chosen when the keypair was generated (`pnpm tauri signer generate`) |

- ⚠️ The minisign **private key lives at `~/tunnel-pilot-updater-keys/` — OUTSIDE the repo.
  NEVER commit it** (org policy + [06 §4](spec/06-MIGRATION-REPO.md)). Only the **public** key
  is committed (in `tauri.conf.json`).
- Losing the private key means future v2 builds can't sign updates that existing installs will
  accept → keep a secure backup of `~/tunnel-pilot-updater-keys/`.
- OS code-signing secrets (`APPLE_*`, Windows Authenticode) are **intentionally NOT set** for
  v2.0 — left commented in the workflow for a future funded maintainer.

---

## 2. RAM measurement (PENDING — device, per OS)

**Methodology** ([01 §5](spec/01-PRD.md), resolved): measure **RSS** of the app process(es)
with the **window HIDDEN after 60s idle**, tray active (the webview is torn down when hidden,
so this captures the true tray-idle footprint). Also record active RAM (3 tunnels connected,
window hidden) and cold-start to tray-ready.

**Targets**: idle **≤ 30 MB** (goal 15–30), active ≤ 60 MB, cold-start ≤ 1.5 s.
How to read RSS: macOS `ps -o rss= -p <pid>` (KB) or Activity Monitor "Memory"; Windows Task
Manager "Working set" / `Get-Process`; Linux `ps -o rss= -p <pid>` or `smem`. Sum all child
processes (webview helper etc.).

| OS | Idle RSS (hidden, 60s, 0 tunnels) | Active RSS (hidden, 3 tunnels) | Cold-start → tray-ready | Installer size |
|----|-----------------------------------|--------------------------------|-------------------------|----------------|
| macOS | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Windows | _TBD_ | _TBD_ | _TBD_ | _TBD_ |
| Linux | _TBD_ | _TBD_ | _TBD_ | _TBD_ |

Record results here (and in `spec/PROGRESS.md`) before tagging the release.

---

## 3. Device/display-gated verifications still owed (PENDING — device)

These cannot be verified headless / in CI. Do them on a real desktop before release:

- **F5 — macOS notifications on an UNSIGNED bundle.** Spike concluded delivery is **best-effort
  and undetectable from Rust** on an unsigned/un-notarized `.app` (show() `Result` is discarded;
  `tauri dev` masks the failure by using `com.apple.Terminal`). **Confirm on a bundled unsigned
  `.app`**: if notifications don't appear, verify the **tray-icon + in-window log/status
  fallback** is sufficient and document it as a known limitation. Acceptance does NOT assume
  macOS notifications work. ([03 §15](spec/03-TECH-SPEC.md))
- **F15 — wake across real system sleep**, per OS: sleep the machine with tunnels connected,
  wake it, confirm the wake watchdog + session-future backstop reconnect. ([03 §4](spec/03-TECH-SPEC.md))
- **Signed self-update end-to-end + tamper rejection.** After a real signed release exists:
  (a) an older v2 install auto-updates to the newer signed build and relaunches; (b) a
  **tampered / wrong-key bundle is rejected** and surfaced as an error (minisign verification).
  ([03 §16](spec/03-TECH-SPEC.md))
- **Runtime desktop behaviors** on a real display: tray icon count + menu + click actions,
  dock/taskbar show-hide following `showInDock`, hide-on-close, single-instance re-show,
  file-picker (identity file + backup), clipboard copy (copy-ssh / logs), macOS transparent
  titlebar + traffic-light inset + drag region, native decorations on Win/Linux.
- **560px responsive breakpoints** in the live webview (container queries verified headless;
  the live min-bounds + reflow want eyes), including ⌘K over a native dialog and drag-reorder
  (HTML5 DnD can't be exercised in jsdom).

---

## 4. v1 bridge release (PENDING — manual, on `master`)

Ship a **final Flutter v1 release** on `master` (e.g. `v1.4.3`) carrying an in-app
**notice/banner** — *"Tunnel Pilot 2.0 is available — here's how to upgrade"* — linking to the
GitHub Releases page + the landing page. It uses v1's existing (unsigned) update path one last
time to **surface the message**; it does **not** auto-install v2 (the update mechanisms are
incompatible). ([06 §6/§7](spec/06-MIGRATION-REPO.md))

- Do this **before or alongside** the v2 release so v1 users are pointed at the v2 download.
- After the first manual hop, all future v2 updates are automatic and signed.

---

## 5. Landing-page install scripts (PENDING — manual, at cutover)

The landing page hero + install tabs still offer the v1 one-liners
`curl … install.sh | bash` and `irm … install.ps1 | iex`. Those scripts (`docs/install.sh`,
`docs/install.ps1`) download the **v1 portable** artifacts and must be updated to fetch the v2
installers (`.dmg` / NSIS `.exe` / `.AppImage`+`.deb`+`.rpm`) — or removed in favor of the
manual-download path — **when the page goes live at cutover**. The static download/upgrade/
workaround copy in `index.html` is already v2-correct.

---

## 6. Actual cutover sequence (PENDING — do NOT do now; manual + CI)

Only when §§1–4 are satisfied and parity (P1–P25 + N1–N6) is confirmed:

1. **Move Flutter to `legacy/flutter/`** (KEEP — do not delete): `lib/`, `macos/`, `windows/`,
   `linux/`, `pubspec.yaml`, `pubspec.lock`, `test/`, `analysis_options.yaml`, Flutter assets.
   The `v1.4.2-flutter-final` tag is the hard archive; `legacy/` keeps it grep-able. ([06 §2](spec/06-MIGRATION-REPO.md))
2. **Relocate / retire the Flutter CI**: `.github/workflows/release.yml` triggers on a `master`
   push that changes `pubspec.yaml` — after the move it won't fire on v2 changes, but relocate
   it under `legacy/` or disable it so it can't accidentally cut a v1 release. `tauri-build.yml`
   currently triggers on `rewrite/tauri`; retarget it to `master` (+ PRs) post-cutover.
3. **Merge `rewrite/tauri` → `master`** (via PR; push through CI, not the CLI).
4. **Tag `v2.0.0`** — this triggers `tauri-release.yml` (matches `v2.*`), which builds all three
   OSes, signs the updater bundles with minisign, publishes a **draft** GitHub Release with
   `latest.json`. Review the draft, then publish so the updater endpoint resolves.
5. Verify `master` builds + releases v2.0.0 from CI; confirm `latest.json` is reachable at the
   configured endpoint.
6. Update `spec/PROGRESS.md` M7 row to done and record the RAM results.

---

## 7. Pre-flight gate summary

- [ ] §1 CI secrets set (`TAURI_UPDATER_PRIVATE_KEY`, `TAURI_UPDATER_KEY_PASSWORD`).
- [ ] §2 RAM measured on all three OSes; idle ≤ 30 MB; table filled in.
- [ ] §3 F5 (mac notif fallback), F15 (wake), signed-update + tamper, runtime desktop behaviors,
      560px breakpoints verified.
- [ ] §4 v1 bridge release shipped with the upgrade notice.
- [ ] §5 landing-page install scripts updated (or removed).
- [ ] §6 Flutter → `legacy/flutter/`; Flutter CI relocated/disabled; merge; tag `v2.0.0`;
      draft release reviewed + published; endpoint verified.

## Post-cutover backlog (not release blockers)

- **F40** [Low] `quit_app` teardown watchdog — add `select!` teardown-vs-timeout(5s)→`exit(0)`
  so a future unbounded engine await can't make the tray app unquittable.
- **F41** [Nit] guard `quit_app` against double-invocation (`AtomicBool`; idempotent).
- **Host-key verification (MITM hardening)** — `ssh/client.rs::check_server_key` returns
  `Ok(true)` (accept-any = v1 `dartssh2` parity, but a carried-forward MITM exposure). Consider
  TOFU / known_hosts-style pinning surfaced in the UI. Revisit post-cutover. (NOTE: the M7 brief
  labeled this "F46", but F46 is already the closed M5 auth-method-validation fix — host-key
  verification has no F-number; tracked as the M1/M3 "host-key backlog".)
- **F50** [Low] `lib/fuzzy.ts` matches over UTF-16 code units → astral/emoji queries can align
  to half a surrogate (imperfect ranking, no crash). Segment by code point only if emoji command
  labels appear.
