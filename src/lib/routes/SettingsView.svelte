<script lang="ts">
  import { save, open } from "@tauri-apps/plugin-dialog";
  import type { AppSettings, ThemeMode } from "../types";
  import { settings } from "../stores/settings";
  import { updateStatus, updateProgress } from "../stores/updater";
  import { importMode } from "../stores/backup";
  import {
    updateSettings,
    exportBackup,
    importBackup,
    checkUpdate,
    installUpdate,
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

  // --- Update banner state (renders all six; updater behavior is M6) ---
  type UpdateState =
    "idle" | "available" | "downloading" | "installing" | "ready" | "error";

  const updateState = $derived<UpdateState>(
    (() => {
      const prog = $updateProgress;
      if (prog) {
        const [, total] = prog;
        return total === null ? "installing" : "downloading";
      }
      const st = $updateStatus;
      if (st?.available && !st.skipped) return "available";
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
    checking = true;
    try {
      const st = await checkUpdate();
      pushToast(
        st.available
          ? `Version ${st.version} available`
          : "You're on the latest version",
        { tone: "info" },
      );
    } catch {
      pushToast("Update checks arrive in a later build", { tone: "info" });
    } finally {
      checking = false;
    }
  }

  async function doInstall(): Promise<void> {
    try {
      await installUpdate();
    } catch {
      pushToast("Updates arrive in a later build", { tone: "info" });
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
        <div class="banner {updateState}" role="status" aria-live="polite">
          {#if updateState === "available"}
            <div class="banner-main">
              <p class="banner-title">
                Version {$updateStatus?.version} available
              </p>
              {#if $updateStatus?.notes}
                <p class="banner-notes">{$updateStatus.notes}</p>
              {/if}
            </div>
            <Button variant="primary" onclick={() => void doInstall()}>
              Download
            </Button>
          {:else if updateState === "downloading"}
            <div class="banner-main">
              <p class="banner-title mono">Downloading… {downloadPct}%</p>
              <div class="progress" aria-hidden="true">
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
              <p class="banner-title">Update ready</p>
            </div>
            <Button variant="primary" onclick={() => void doInstall()}>
              Restart to update
            </Button>
          {:else if updateState === "error"}
            <div class="banner-main">
              <p class="banner-title">Update failed</p>
            </div>
            <Button onclick={() => void checkNow()}>Retry</Button>
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
    gap: var(--sp-4);
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
    min-width: 0;
    flex: 1;
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
    color: var(--text-2);
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
