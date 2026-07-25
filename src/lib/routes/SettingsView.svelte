<script lang="ts">
  import { save, open } from "@tauri-apps/plugin-dialog";
  import type { AppSettings, ThemeMode } from "../types";
  import { settings } from "../stores/settings";
  import {
    updateStatus,
    updateProgress,
    clearUpdateProgress,
    toUpdateErrorMessage,
  } from "../stores/updater";
  import { importMode } from "../stores/backup";
  import { activeView } from "../ui/view";
  import {
    updateSettings,
    exportBackup,
    importBackup,
    checkUpdate,
    installUpdate,
    skipUpdate,
  } from "../ipc";
  import { pushToast } from "../ui/toast";
  import Toggle from "../components/ui/Toggle.svelte";
  import Select from "../components/ui/Select.svelte";
  import SegmentedControl from "../components/ui/SegmentedControl.svelte";
  import Button from "../components/ui/Button.svelte";
  import Icon from "../components/ui/Icon.svelte";
  import Dialog from "../components/ui/Dialog.svelte";

  const VERSION = "2.0.0";

  let checking = $state(false);
  let updateError = $state<string | null>(null);
  let importPath = $state<string | null>(null);

  // Settings write: send the whole struct with one field changed. The store is
  // source of truth and only advances on the backend's settings://changed, so a
  // failed write leaves the control reflecting the old value (auto-revert).
  async function patch(partial: Partial<AppSettings>): Promise<void> {
    const cur = $settings;
    if (!cur) return;
    try {
      await updateSettings({ ...cur, ...partial });
    } catch (err) {
      pushToast(`Couldn't save setting: ${String(err)}`, { tone: "error" });
    }
  }

  const retryOptions = [1, 2, 3, 5, 8, 10].map((n) => ({
    value: n,
    label: String(n),
  }));
  const delayOptions = [1, 2, 3, 5, 10, 30].map((n) => ({
    value: n,
    label: `${n}s`,
  }));

  // --- Update banner state machine (spec §8) ---
  //
  // State is derived from live store signals plus two ephemeral local flags:
  //   - `checking`     — a `check_update()` call is in flight
  //   - `updateError`  — install-action fallback: `install_update` may still
  //                      THROW, so its rejection is coerced (via
  //                      `toUpdateErrorMessage`, never an object) into a readable
  //                      string here. Check failures no longer throw.
  //
  // The primary error source is `$updateStatus.error`: `check_update` (and the
  // tray "Check for Updates") now RETURN Ok(status) with any failure in
  // `status.error` (a clean human-readable string) and emit it on
  // `update://status`. Silent startup checks leave it null, so a benign
  // fresh-launch "no release yet" never raises a scary red banner. Either source
  // is a string — "[object Object]" can never reach the UI.
  // The backend never re-emits `update://status` on skip (it only persists
  // `lastSkippedVersion` + emits `settings://changed`), so `available` also
  // reconciles against `settings.lastSkippedVersion` — skipping a version hides
  // the banner as soon as the settings store advances.
  //
  // `install_update()` downloads (→ `update://progress`) then relaunches the app
  // itself on success, so there is no user-actionable "ready-then-restart" step:
  // `ready` is the download-complete → relaunching moment (no button), and a
  // failed install lands in `error`.
  type UpdateState =
    | "idle"
    | "checking"
    | "available"
    | "downloading"
    | "installing"
    | "ready"
    | "error";

  // Readable error message, event first (`status.error`) then install fallback.
  // Both are already plain strings; a blank one is treated as "no error".
  const errorMessage = $derived<string | null>(
    (() => {
      const fromStatus = $updateStatus?.error;
      if (typeof fromStatus === "string" && fromStatus.trim() !== "") {
        return fromStatus;
      }
      return updateError;
    })(),
  );

  const updateState = $derived<UpdateState>(
    (() => {
      // An in-flight check wins over a stale `status.error` from the previous
      // attempt, so Retry/Check now shows "Checking…" rather than the old error.
      if (checking) return "checking";
      const prog = $updateProgress;
      if (prog) {
        const [downloaded, total] = prog;
        if (total === null) return "installing";
        if (total > 0 && downloaded >= total) return "ready";
        return "downloading";
      }
      // Only a genuine, message-bearing error goes to the `error` state; an
      // empty/absent message stays idle rather than showing "Update failed".
      if (errorMessage) return "error";
      const st = $updateStatus;
      const skippedVersion = $settings?.lastSkippedVersion ?? null;
      if (
        st?.available &&
        st.version !== null &&
        !st.skipped &&
        st.version !== skippedVersion
      ) {
        return "available";
      }
      return "idle";
    })(),
  );

  const downloadPct = $derived(
    (() => {
      const prog = $updateProgress;
      if (!prog || prog[1] === null || prog[1] === 0) return 0;
      return Math.min(100, Math.round((prog[0] / prog[1]) * 100));
    })(),
  );

  async function checkNow(): Promise<void> {
    clearUpdateProgress();
    updateError = null;
    checking = true;
    try {
      const st = await checkUpdate();
      // `update://status` drives the banner; only a clean up-to-date result
      // needs a toast (there is no banner state for "no update found"). A failed
      // check now returns Ok(status) with `status.error` set → the banner shows
      // the error, so suppress the "latest version" toast in that case.
      if (!st.available && !st.error) {
        pushToast("You're on the latest version", { tone: "info" });
      }
    } catch (err) {
      // Fallback only: check_update no longer throws, but stay defensive.
      updateError = toUpdateErrorMessage(err) || null;
    } finally {
      checking = false;
    }
  }

  async function doInstall(): Promise<void> {
    clearUpdateProgress();
    updateError = null;
    try {
      // On success the backend verifies + installs + relaunches the app, so this
      // promise typically never resolves here — progress events drive the UI and
      // the process restarts. A rejection means the install/verify failed.
      await installUpdate();
    } catch (err) {
      updateError = toUpdateErrorMessage(err) || null;
      clearUpdateProgress();
    }
  }

  async function doSkip(): Promise<void> {
    const version = $updateStatus?.version;
    if (!version) return;
    try {
      // Persists `lastSkippedVersion`; the resulting settings://changed advances
      // the settings store, which drops the banner back to `idle`.
      await skipUpdate(version);
    } catch (err) {
      pushToast(`Couldn't skip this version: ${String(err)}`, {
        tone: "error",
      });
    }
  }

  // --- Backup ---
  async function doExport(): Promise<void> {
    try {
      const path = await save({
        defaultPath: "tunnel-pilot-backup.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (!path) return;
      await exportBackup(path);
      pushToast("Configuration exported", { tone: "success" });
    } catch (err) {
      pushToast(`Export failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function pickImport(): Promise<void> {
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
        title: "Select a backup file",
      });
      if (typeof picked === "string") importPath = picked;
    } catch (err) {
      pushToast(`Could not open file picker: ${String(err)}`, {
        tone: "error",
      });
    }
  }

  async function confirmImport(): Promise<void> {
    if (!importPath) return;
    try {
      const result = await importBackup(importPath, $importMode);
      pushToast(
        `Imported ${result.imported} tunnel${result.imported === 1 ? "" : "s"}` +
          (result.skipped > 0 ? `, skipped ${result.skipped}` : ""),
        { tone: "success" },
      );
    } catch (err) {
      pushToast(`Import failed: ${String(err)}`, { tone: "error" });
    } finally {
      importPath = null;
    }
  }
</script>

<section class="view">
  <div class="scroll">
    <div class="col">
      <h1 class="title">Settings</h1>

      <!-- Update banner (spec §8) -->
      {#if updateState !== "idle"}
        <div
          class="banner {updateState}"
          role={updateState === "error" ? "alert" : "status"}
          aria-live={updateState === "error" ? "assertive" : "polite"}
        >
          {#if updateState === "checking"}
            <div class="banner-main">
              <p class="banner-title">Checking for updates…</p>
              <div class="progress indet" aria-hidden="true">
                <span class="bar"></span>
              </div>
            </div>
          {:else if updateState === "available"}
            <div class="banner-main">
              <p class="banner-title">
                Version {$updateStatus?.version} available
              </p>
              {#if $updateStatus?.notes}
                <details class="notes">
                  <summary>Release notes</summary>
                  <p class="banner-notes">{$updateStatus.notes}</p>
                </details>
              {/if}
            </div>
            <div class="banner-actions">
              <Button
                variant="ghost"
                size="sm"
                onclick={() => void doSkip()}
              >
                Skip this version
              </Button>
              <Button
                variant="primary"
                size="sm"
                iconLeft="download"
                onclick={() => void doInstall()}
              >
                Install &amp; restart
              </Button>
            </div>
          {:else if updateState === "downloading"}
            <div class="banner-main">
              <p class="banner-title mono">Downloading… {downloadPct}%</p>
              <div
                class="progress"
                role="progressbar"
                aria-label="Update download progress"
                aria-valuenow={downloadPct}
                aria-valuemin={0}
                aria-valuemax={100}
              >
                <span class="bar" style="width: {downloadPct}%"></span>
              </div>
            </div>
          {:else if updateState === "installing"}
            <div class="banner-main">
              <p class="banner-title">Installing…</p>
              <div class="progress indet" aria-hidden="true">
                <span class="bar"></span>
              </div>
            </div>
          {:else if updateState === "ready"}
            <div class="banner-main">
              <p class="banner-title">Update ready — restarting…</p>
              <div class="progress indet" aria-hidden="true">
                <span class="bar"></span>
              </div>
            </div>
          {:else if updateState === "error"}
            <div class="banner-main">
              <p class="banner-title">Update failed</p>
              {#if errorMessage}
                <p class="banner-notes">{errorMessage}</p>
              {/if}
            </div>
            <div class="banner-actions">
              <button
                type="button"
                class="link"
                onclick={() => activeView.set("activity")}
              >
                View log
              </button>
              <Button variant="primary" size="sm" onclick={() => void checkNow()}>
                Retry
              </Button>
            </div>
          {/if}
        </div>
      {/if}

      {#if $settings}
        {@const s = $settings}
        <!-- STARTUP -->
        <section class="group">
          <h2 class="overline">Startup</h2>
          <div class="rows">
            <div class="setting">
              <div class="s-text">
                <span class="s-label">Launch at login</span>
              </div>
              <Toggle
                checked={s.launchAtLogin}
                ariaLabel="Launch at login"
                onchange={(v) => void patch({ launchAtLogin: v })}
              />
            </div>
            <div class="setting">
              <div class="s-text">
                <span class="s-label">Show in Dock / taskbar</span>
              </div>
              <Toggle
                checked={s.showInDock}
                ariaLabel="Show in Dock or taskbar"
                onchange={(v) => void patch({ showInDock: v })}
              />
            </div>
          </div>
        </section>

        <!-- CONNECTIONS -->
        <section class="group">
          <h2 class="overline">Connections</h2>
          <div class="rows">
            <div class="setting">
              <div class="s-text">
                <span class="s-label">Desktop notifications</span>
                <span class="s-sub"
                  >Notify on connect, disconnect, and errors.</span
                >
              </div>
              <Toggle
                checked={s.showNotifications}
                ariaLabel="Desktop notifications"
                onchange={(v) => void patch({ showNotifications: v })}
              />
            </div>
            <div class="setting">
              <div class="s-text">
                <span class="s-label">Auto-reconnect</span>
                <span class="s-sub">
                  Retries: {s.autoReconnectMaxRetries} · Delay:
                  {s.autoReconnectDelaySec}s
                </span>
              </div>
              <Toggle
                checked={s.autoReconnect}
                ariaLabel="Auto-reconnect"
                onchange={(v) => void patch({ autoReconnect: v })}
              />
            </div>
            <div class="sub-options" class:open={s.autoReconnect}>
              <div class="sub-inner">
                <div class="sub-row">
                  <label class="s-label" for="rc-retries">Retries</label>
                  <Select
                    id="rc-retries"
                    ariaLabel="Reconnect retries"
                    value={s.autoReconnectMaxRetries}
                    options={retryOptions}
                    disabled={!s.autoReconnect}
                    onchange={(v) => void patch({ autoReconnectMaxRetries: v })}
                  />
                </div>
                <div class="sub-row">
                  <label class="s-label" for="rc-delay">Delay</label>
                  <Select
                    id="rc-delay"
                    ariaLabel="Reconnect delay"
                    value={s.autoReconnectDelaySec}
                    options={delayOptions}
                    disabled={!s.autoReconnect}
                    onchange={(v) => void patch({ autoReconnectDelaySec: v })}
                  />
                </div>
              </div>
            </div>
          </div>
        </section>

        <!-- UPDATES -->
        <section class="group">
          <h2 class="overline">Updates</h2>
          <div class="rows">
            <div class="setting">
              <div class="s-text">
                <span class="s-label">Automatically check for updates</span>
              </div>
              <Toggle
                checked={s.autoCheckUpdates}
                ariaLabel="Automatically check for updates"
                onchange={(v) => void patch({ autoCheckUpdates: v })}
              />
            </div>
            <div class="setting inline-action">
              <span class="s-sub">Check the release channel now.</span>
              <Button
                size="sm"
                iconLeft="refresh-cw"
                loading={checking}
                onclick={() => void checkNow()}
              >
                Check now
              </Button>
            </div>
          </div>
        </section>

        <!-- APPEARANCE -->
        <section class="group">
          <h2 class="overline">Appearance</h2>
          <div class="rows">
            <div class="setting">
              <div class="s-text">
                <span class="s-label">Theme</span>
              </div>
              <SegmentedControl
                value={s.themeMode}
                ariaLabel="Theme"
                compact
                options={[
                  { value: "system", label: "System", icon: "monitor" },
                  { value: "light", label: "Light", icon: "sun" },
                  { value: "dark", label: "Dark", icon: "moon" },
                ]}
                onchange={(v) => void patch({ themeMode: v as ThemeMode })}
              />
            </div>
          </div>
        </section>

        <!-- BACKUP & RESTORE -->
        <section class="group">
          <h2 class="overline">Backup &amp; restore</h2>
          <div class="rows">
            <button
              type="button"
              class="setting clickable"
              onclick={() => void doExport()}
            >
              <div class="s-text">
                <span class="s-label">Export configuration</span>
                <span class="s-sub">
                  Exports exclude passwords; identity-file paths are kept.
                </span>
              </div>
              <span class="row-cta"
                ><Icon name="download" size={16} /> Export</span
              >
            </button>

            <div class="setting import-mode">
              <div class="s-text">
                <span class="s-label">Import mode</span>
                <span class="s-sub">
                  {$importMode === "replace"
                    ? "Replace removes all current tunnels first."
                    : "Merge adds new tunnels and keeps existing ones."}
                </span>
              </div>
              <SegmentedControl
                value={$importMode}
                ariaLabel="Import mode"
                options={[
                  { value: "merge", label: "Merge" },
                  { value: "replace", label: "Replace" },
                ]}
                onchange={(v) => importMode.set(v)}
              />
            </div>

            <button
              type="button"
              class="setting clickable"
              onclick={() => void pickImport()}
            >
              <div class="s-text">
                <span class="s-label">Import configuration</span>
              </div>
              <span class="row-cta"
                ><Icon name="upload" size={16} /> Import</span
              >
            </button>
          </div>
        </section>
      {/if}

      <footer class="footer">
        <span class="version mono">Tunnel Pilot v{VERSION}</span>
        <button type="button" class="link" onclick={() => void checkNow()}>
          check for updates
        </button>
      </footer>
    </div>
  </div>
</section>

{#if importPath}
  <Dialog
    title="Import configuration?"
    size="sm"
    onClose={() => (importPath = null)}
  >
    <p class="dlg-body">
      {#if $importMode === "replace"}
        <strong>Replace</strong> will remove all current tunnels, then import the
        backup. Live tunnels are disconnected first. This can't be undone.
      {:else}
        <strong>Merge</strong> will add new tunnels from the backup and keep your
        existing ones. Duplicates are skipped.
      {/if}
    </p>
    <p class="dlg-path mono">{importPath}</p>
    {#snippet footer()}
      <Button onclick={() => (importPath = null)}>Cancel</Button>
      <Button
        variant={$importMode === "replace" ? "danger" : "primary"}
        onclick={() => void confirmImport()}
      >
        {$importMode === "replace" ? "Replace & import" : "Import"}
      </Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .view {
    height: 100%;
    overflow: hidden;
  }
  .scroll {
    height: 100%;
    overflow-y: auto;
    padding: var(--sp-5) var(--sp-6) var(--sp-9);
  }
  .col {
    max-width: 640px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: var(--sp-7);
  }
  .title {
    margin: 0;
    font-size: var(--fs-title-lg);
    line-height: var(--lh-title-lg);
    font-weight: var(--fw-title-lg);
    letter-spacing: var(--tracking-tight);
  }
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .rows {
    display: flex;
    flex-direction: column;
  }
  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
    padding: var(--sp-4) 0;
    border-bottom: var(--border-w) solid var(--divider);
  }
  .setting:last-child {
    border-bottom: none;
  }
  .inline-action {
    justify-content: space-between;
  }
  .s-text {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  .s-label {
    font-size: var(--fs-title-sm);
    line-height: var(--lh-title-sm);
    font-weight: var(--fw-title-sm);
    color: var(--text);
  }
  .s-sub {
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }

  /* Auto-reconnect sub-options: animate open (grid-rows trick → reduced-motion
     safe, the global rule zeroes the transition). */
  .sub-options {
    display: grid;
    grid-template-rows: 0fr;
    transition: grid-template-rows var(--dur-fast) var(--ease-standard);
  }
  .sub-options.open {
    grid-template-rows: 1fr;
  }
  .sub-inner {
    overflow: hidden;
    display: flex;
    gap: var(--sp-7);
    padding-left: var(--sp-5);
  }
  .sub-options.open .sub-inner {
    padding-top: var(--sp-2);
    padding-bottom: var(--sp-4);
  }
  .sub-row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }

  .clickable {
    width: 100%;
    border: none;
    border-bottom: var(--border-w) solid var(--divider);
    background: transparent;
    text-align: left;
    cursor: pointer;
    border-radius: var(--radius-sm);
    transition: background-color var(--dur-fast) var(--ease-standard);
  }
  .clickable:hover {
    background: var(--hover);
  }
  .row-cta {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    flex: none;
    color: var(--accent-text);
    font-size: var(--fs-body);
    font-weight: 500;
  }

  .banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--sp-3) var(--sp-4);
    padding: var(--sp-4) var(--sp-5);
    border-radius: var(--radius-md);
    border: var(--border-w) solid var(--border);
  }
  .banner.available {
    background: var(--accent-subtle);
    border-color: transparent;
  }
  .banner.ready {
    background: var(--status-connected-bg);
    border-color: transparent;
  }
  .banner.error {
    background: var(--status-error-bg);
    border-color: transparent;
  }
  .banner-main {
    min-width: 12rem;
    flex: 1 1 12rem;
  }
  .banner-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex: none;
  }
  .banner-title {
    margin: 0;
    font-size: var(--fs-title-sm);
    font-weight: var(--fw-title-sm);
    color: var(--text);
  }
  .banner-notes {
    margin: var(--sp-1) 0 0;
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
    white-space: pre-line;
  }
  .notes {
    margin-top: var(--sp-1);
  }
  .notes summary {
    display: inline-flex;
    align-items: center;
    font-size: var(--fs-body-sm);
    color: var(--accent-text);
    cursor: pointer;
    list-style: none;
    border-radius: var(--radius-sm);
  }
  .notes summary::-webkit-details-marker {
    display: none;
  }
  .notes summary:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
  }
  .notes summary:hover {
    text-decoration: underline;
  }
  .progress {
    margin-top: var(--sp-2);
    height: 4px;
    border-radius: var(--radius-full);
    background: var(--surface-3);
    overflow: hidden;
  }
  .progress .bar {
    display: block;
    height: 100%;
    background: var(--accent);
    border-radius: var(--radius-full);
    transition: width var(--dur-base) var(--ease-standard);
  }
  .progress.indet .bar {
    width: 40%;
    animation: indet 1.2s ease-in-out infinite;
  }
  @keyframes indet {
    0% {
      transform: translateX(-100%);
    }
    100% {
      transform: translateX(320%);
    }
  }

  .footer {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding-top: var(--sp-4);
    border-top: var(--border-w) solid var(--divider);
  }
  .version {
    font-size: var(--fs-mono-sm);
    color: var(--text-3);
  }
  .link {
    border: none;
    background: transparent;
    color: var(--accent-text);
    font-size: var(--fs-body-sm);
    cursor: pointer;
    padding: 0;
  }
  .link:hover {
    text-decoration: underline;
  }
  .dlg-body {
    margin: 0 0 var(--sp-3);
    font-size: var(--fs-body);
    line-height: var(--lh-body);
    color: var(--text);
  }
  .dlg-path {
    margin: 0 0 var(--sp-2);
    font-size: var(--fs-mono-sm);
    color: var(--text-2);
    word-break: break-all;
  }
  @media (prefers-reduced-motion: reduce) {
    .progress.indet .bar {
      animation: none;
      width: 100%;
    }
  }
</style>
