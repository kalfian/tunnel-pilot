# 06 — Repo & Release Migration

> How the Flutter→Tauri rewrite coexists in one repo, how we cut over, how CI/CD changes,
> and how existing v1 users are brought across (incompatible updaters).
> Cross-refs: [01-PRD.md](01-PRD.md), [03-TECH-SPEC.md](03-TECH-SPEC.md),
> [07-ROADMAP.md](07-ROADMAP.md), [AGENTS.md](AGENTS.md).

## 1. Branch & tag strategy (locked)

- `master` — current Flutter v1.4.2. **Stays authoritative until cutover.** Only critical
  v1 fixes + the "bridge" release (§6) land here pre-cutover.
- `rewrite/tauri` — all v2 development. Branched from `master`.
- `v1.4.2-flutter-final` — tag already created; archives the last Flutter build so it is
  recoverable after cutover regardless of what happens to the working tree.
- **Cutover** = merge `rewrite/tauri` → `master` once v2 reaches feature parity (P1–P25 +
  N1–N6 pass, see [01](01-PRD.md) / [07 M7](07-ROADMAP.md)).

Project rule (from vault): **push happens via the CI workflow, not `git push` from CLI.**
Commits are made locally; pushing/releasing goes through GitHub Actions. Keep that here.

## 2. Directory transition plan

**During development (on `rewrite/tauri`):** Tauri lives alongside Flutter so the repo
still builds v1 from `master` and v2 from the branch.

```
tunnel-pilot/
  lib/                # Flutter (v1) — untouched on rewrite/tauri
  macos/ windows/ linux/   # Flutter platform runners (v1)
  pubspec.yaml        # Flutter (v1)
  src/                # NEW: Svelte frontend (v2)
  src-tauri/          # NEW: Rust + Tauri (v2)
  package.json        # NEW: frontend deps + tauri scripts
  vite.config.ts      # NEW
  spec/               # these docs
  docs/index.html     # landing page (shared; update at cutover)
```

**At cutover (merge to `master`):** move Flutter into `legacy/flutter/` (recoverable, out
of the way) rather than deleting — the `v1.4.2-flutter-final` tag is the hard archive, but a
`legacy/` dir keeps it grep-able during the transition. Decision to fully delete `legacy/`
deferred until v2 is proven in the wild (a few releases). Proposed layout post-cutover:

```
tunnel-pilot/
  legacy/flutter/     # former lib/, macos/, windows/, linux/, pubspec.yaml
  src/                # Svelte
  src-tauri/          # Rust/Tauri
  package.json vite.config.ts
  docs/index.html
```

**Root hygiene:** the `.gitignore` must cover `src-tauri/target/`, `node_modules/`,
`dist/`, and (per org policy) never commit the updater private key or any secrets file.

## 3. Cutover checklist

- [ ] All parity acceptance criteria in [03](03-TECH-SPEC.md) pass on all three OSes.
- [ ] N1–N6 (command palette, resizable, groups/tags, keychain, signed updater) done.
- [ ] RAM idle target (15–30 MB) verified with documented methodology ([01 §5](01-PRD.md)).
- [ ] v1→v2 migration verified against a real v1 `tunnel_pilot_config.json` ([04 §12](04-DATA-MODEL.md)).
- [ ] Signed release artifacts produced by CI for macOS/Windows/Linux; signature verified.
- [ ] Updater `latest.json` published to the same GitHub Releases; a v2→v2 self-update works end to end.
- [ ] Bridge plan for v1 users executed or scheduled (§6).
- [ ] Landing page (`docs/index.html`) updated (§7).
- [ ] Flutter moved to `legacy/flutter/`; build/test docs updated ([AGENTS.md](AGENTS.md)).
- [ ] `master` builds and releases v2 from CI; version bumped to `2.0.0`.

## 4. CI/CD changes (GitHub Actions)

Replace the Flutter build workflow with a Tauri cross-platform build + sign + release
workflow. Keep publishing to the **same GitHub repo Releases** so the updater endpoint is
stable for future v2 users.

Proposed `.github/workflows/release.yml` outline:

```yaml
name: release
on:
  push:
    tags: ["v*"]              # release on version tags
jobs:
  build:
    strategy:
      matrix:
        include:
          - { os: macos-latest,   target: universal-apple-darwin }
          - { os: windows-latest, target: x86_64-pc-windows-msvc }
          - { os: ubuntu-latest,  target: x86_64-unknown-linux-gnu }
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: pnpm/action-setup@v4      # or npm
      - uses: actions/setup-node@v4
      - run: pnpm install
      # Linux: apt install webkit2gtk, libssl, etc. (tauri deps)
      - name: Build + sign updater bundle (tauri-action)
        uses: tauri-apps/tauri-action@v0
        env:
          # Updater BUNDLE signing (minisign) — REQUIRED, free (self-generated keypair):
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_UPDATER_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_UPDATER_KEY_PASSWORD }}
          # --- OS code-signing / notarization: STUBBED / DISABLED for v2.0 ---
          # These are intentionally NOT set. The project is open-source and unfunded, and
          # OS signing certs cost money (Apple Developer ID ~$99/yr, Windows cert ~$100s/yr).
          # Leave the env keys here, commented, so a future funded maintainer can enable
          # signing by populating these secrets — no workflow restructuring needed:
          # APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          # APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          # APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          # APPLE_ID: ${{ secrets.APPLE_ID }}
          # APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          # APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
          # (Windows Authenticode signing similarly left unset.)
        with:
          tagName: ${{ github.ref_name }}
          releaseDraft: true
          includeUpdaterJson: true       # emits latest.json for tauri-plugin-updater
```

**Two distinct signing tiers — keep them straight:**

| Tier | What it protects | v2.0 status | Cost |
|------|------------------|-------------|------|
| **Updater bundle signing (minisign)** | Self-update integrity (verified against pubkey embedded in app) | **REQUIRED / done** | Free — self-generated keypair, no cert authority |
| **OS code-signing / notarization** (Apple Developer ID, Windows Authenticode) | Gatekeeper/SmartScreen at install time | **SKIPPED** (deliberate baseline) | Costs money — paid certs we don't have |

Contrast plainly: **update-bundle signing = free, done; OS code-signing = costs money, skipped.**

Key points:
- **Updater minisign key**: generate with `pnpm tauri signer generate`. Public key goes in
  `tauri.conf.json` (`plugins.updater.pubkey`); private key + password are **GitHub Actions
  secrets only**, never in the repo (org policy: no secrets committed). See
  [03 §updater](03-TECH-SPEC.md#updater). This tier is enforced from day one.
- **macOS OS-signing/notarization**: **not done for v2.0** — the project is open-source and
  unfunded; Developer ID + notarization require a paid Apple account (~$99/yr). App is
  unsandboxed ([03 §platform](03-TECH-SPEC.md#platform)). Users bypass Gatekeeper via
  **right-click → Open** (copy-ready text in §8, README/landing).
- **Windows OS-signing**: **not done for v2.0** — OV/EV code-signing certs cost money.
  Users bypass SmartScreen via **"More info → Run anyway"** (copy-ready text in §8).
- Enabling either OS-signing tier later is **optional future enablement** (if funded /
  donations), not a planned milestone — the CI env keys above are left commented so it's a
  drop-in.
- **Linux**: AppImage + `.deb`/`.rpm` as configured; webkit2gtk deps installed in CI (no
  code-signing concept in the same sense).
- `latest.json` (updater manifest) is published to the release so `check_update` resolves.
- CI is the **only** push/release path (project rule). Tagging triggers the release.

## 5. Versioning

- v2 starts at **`2.0.0`**. `Cargo.toml`, `package.json`, and `tauri.conf.json` versions
  must stay in sync (single source: a small script or `tauri.conf.json` as canonical).
- The `v1.4.2-flutter-final` tag remains the last v1. v2 tags are `v2.x.y`.

## 6. Upgrade path per OS & bringing v1 users across

**Problem**: v1's Flutter self-updater (raw GitHub HttpClient + inline install scripts) and
v2's `tauri-plugin-updater` (minisign-signed bundles) are **incompatible update mechanisms**.
A v1 install cannot auto-swap itself into v2 — different bundle format, different update
protocol, v1 has no signature machinery. So the first hop v1→v2 is **manual**, and the shape
of "manual" differs per OS. Auto-uninstall of v1 is mostly infeasible; frame it honestly.

### THE KEY UX GUARANTEE — config is NOT lost
The user's tunnels survive the upgrade because **the config lives in the OS app-support dir,
OUTSIDE the app bundle** — replacing/deleting the app never touches it. On first launch v2
detects and imports the v1 config via the **hardcoded per-OS v1-path probe** in
[04 §12](04-DATA-MODEL.md) (this is required precisely because the v1 macOS bundle id
`com.kalfian.tunnelpilot` and Windows `%APPDATA%\kalfian\Tunnel Pilot\` do not match v2's
`app_config_dir`). Passwords move into the OS keychain automatically. **Make this guarantee
prominent in release notes, README, and the landing page.**

### Per-OS upgrade reality (F8)
- **macOS — in-place overwrite (de-facto auto-uninstall).** The only feasible path: v2 uses
  the **same app display name** and installs to the **same location `/Applications/<Name>.app`**,
  so dragging the v2 `.dmg` over `/Applications` overwrites v1 in place. Tauri v2 has **no
  `.pkg` target** and a `.dmg` **cannot run scripts**, so a preinstall-script uninstall is not
  available. Requires: (a) resolving the F2 bundle-id/paths (config still found via the probe
  regardless of bundle id), and (b) matching v1's `.app` display name ("Tunnel Pilot"). See
  the login-item caveat (F18) below.
- **Windows — auto-uninstall INFEASIBLE.** v1 shipped as a **portable `.zip`** (verified in
  `lib/services/update_service.dart`: asset `-windows.zip`, installed by `Expand-Archive` +
  `xcopy` into the app dir) — **no installer, no uninstaller, no `Uninstall\...` registry
  entry**. So an NSIS `installerHooks` / WiX `UpgradeCode` has nothing to detect and cannot
  remove v1. Document a **manual step**: "delete your old Tunnel Pilot portable folder"; the
  v2 installer installs cleanly alongside, and single-instance ensures only one runs. Config
  is untouched by this (guarantee above).
- **Linux — N/A (F17).** v1 **never shipped a Linux artifact**, so there is no v1 app to
  uninstall and no v1 config to migrate. Linux = **fresh install only**.

### F18 — macOS login-item orphaning
v1 used **`SMAppService.mainApp`** for launch-at-login; v2's `tauri-plugin-autostart` uses the
**`auto-launch` crate (AppleScript "Login Items")** — a *different* mechanism. After an
in-place overwrite, v1's `SMAppService` login registration may be **orphaned** (a stale login
item). Handle it: on v2 **first run**, if `launchAtLogin` is on, (re)register via
`tauri-plugin-autostart` and document that a user may need to remove a leftover "Tunnel Pilot"
entry in System Settings → General → Login Items if a duplicate appears. Note this in release
notes.

### Notification channels for the upgrade (user requirement)
1. **Final v1 "bridge" release** on `master` (e.g. `v1.4.3`) with an in-app **notice/banner**:
   *"Tunnel Pilot 2.0 is available — here's how to upgrade"* linking to the releases page +
   landing page. Uses v1's existing (unsigned) update path one last time to *surface the
   message*; it does **not** auto-install v2.
2. **Landing page** (`docs/index.html`, §7) + **README** upgrade steps, per-OS as above,
   combined with the Gatekeeper/SmartScreen unsigned workarounds (§8).
3. After the first manual hop, **future v2 updates are automatic and signed** (minisign
   bundles) via `tauri-plugin-updater`, same GitHub Releases endpoint.

**Bundle identifier note**: choose the v2 macOS bundle id deliberately (it may differ from
v1's `com.kalfian.tunnelpilot`). Config discovery does **not** depend on it — the migration
probe hardcodes the v1 paths ([04 §12](04-DATA-MODEL.md)). Matching v1's `.app` **display
name** is what enables the in-place macOS overwrite, not the bundle id.

## 7. Landing page (`docs/index.html`, GitHub Pages)

- Update download links/buttons to the v2 installers (macOS `.dmg`/Windows `.zip` or
  installer/Linux AppImage+deb+rpm).
- Add an "Upgrading from v1?" section, **per OS** (§6): macOS = drag the new `.dmg` over
  `/Applications` (overwrites in place); Windows = delete your old portable folder, then run
  v2; Linux = fresh install (v1 never shipped on Linux).
- **Lead with the guarantee**: "Your saved tunnels are preserved — config lives outside the
  app and v2 imports it automatically on first launch; passwords move into your OS keychain."
- **Include the install workarounds from §8 prominently** near the download buttons (the app
  is unsigned — users WILL hit Gatekeeper/SmartScreen on first launch).
- Refresh RAM/positioning copy to reflect the lean footprint ([01 §5](01-PRD.md)).
- Keep the page in `docs/` so GitHub Pages continues to serve it; update at cutover (§3).

## 8. Install workarounds — copy-ready for README + landing (§7)

The app ships **unsigned at the OS level** (open-source, unfunded — see §4). These are the
**permanent** first-launch instructions users need, not a temporary note. Copy verbatim to
`README.md` and `docs/index.html`.

> **Tunnel Pilot is open-source and not code-signed** (signing certificates cost money this
> project doesn't have). Your OS may warn on first launch — this is expected and safe. Here's
> how to open it:
>
> **macOS** — Gatekeeper will say the app "cannot be opened because the developer cannot be
> verified." Right-click (or Control-click) the app in Finder → **Open** → **Open** in the
> dialog. You only need to do this once. (Or: System Settings → Privacy & Security → "Open
> Anyway".)
>
> **Windows** — SmartScreen will show "Windows protected your PC." Click **More info** →
> **Run anyway**.
>
> **Linux** — AppImage: `chmod +x Tunnel-Pilot*.AppImage` then run it. `.deb`/`.rpm` install
> normally.
>
> Note: this only affects the *installer/first launch*. **Auto-updates are cryptographically
> signed and verified** (minisign) regardless — that protection is always on.

## 9. Open items for human (with resolved defaults)

- **OS code-signing / notarization** — RESOLVED: **skipped for v2.0**, deliberate baseline
  (open-source + unfunded; certs cost money). CI hooks stubbed for optional future
  enablement if funded/donated (§4). Not a planned milestone.
- **Apple notarization credential provisioning** — deferred; only relevant if/when signing is
  funded. No action for v2.0.
- **`legacy/flutter/` retention** — RESOLVED (user default): **keep for a few releases**, do
  not delete at cutover (§2).
- **RAM measurement methodology** — RESOLVED: measure RSS of the app process(es) with the
  window **HIDDEN after 60s idle**, tray active, on each OS (webview torn down when hidden).
  Applied as the M7 gate ([07](07-ROADMAP.md), [01 §5](01-PRD.md)).
