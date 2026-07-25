<!--
  TrayPopover — the tray-icon popover panel (spec 05 §"tray" showcase in
  docs/index.html). Rendered INSTEAD of the main app when the webview label is
  `tray_popover` (see main.ts). Pixel-matches the landing mockup with real,
  live data:

    [ ↑ Update available · vX.Y.Z ]        (only when an update is available)
    PRODUCTION                             (uppercase, group-colored)
      ● Postgres                    :5432
      ● Redis                       :6379
    UNGROUPED
      ○ Kibana                      :5601
    ─────────────────────────────────────
    ▶ Start all   ⚙ Settings   ⏻ Quit

  Rows toggle connect/disconnect (retry on error) via lib/ipc; the whole panel
  is a rounded, transparent-window surface. hydrateAll() runs on mount and on
  every `tray://opened` (wired in hydrate.ts) so state is always fresh. Esc
  dismisses via hideTrayPopover().
-->
<script lang="ts">
  import type { ForwardConfig, ForwardStatus } from "../types";
  import { forwards, statusById, lastErrorById } from "../stores/forwards";
  import { groups } from "../stores/groups";
  import { updateStatus } from "../stores/updater";
  import {
    connectForward,
    disconnectForward,
    retryForward,
    startAll,
    showWindow,
    hideTrayPopover,
    quitApp,
  } from "../ipc";
  import { hydrateAll } from "../hydrate";
  import { groupColorVar } from "../ui/groupColors";
  import StatusDot from "./ui/StatusDot.svelte";
  import Icon from "./ui/Icon.svelte";

  let listEl = $state<HTMLDivElement | undefined>();

  // Always re-pull live state when the panel first mounts (each open also fires
  // `tray://opened` → hydrateAll via hydrate.ts). The webview holds no truth.
  $effect(() => {
    void hydrateAll();
  });

  const updateAvailable = $derived(
    $updateStatus?.available === true && $updateStatus?.skipped !== true,
  );

  interface Section {
    id: string;
    label: string;
    color: string | null;
    members: ForwardConfig[];
  }

  // Groups (in order) with ≥1 member, then Ungrouped. When there are no groups
  // at all, a single unlabeled flat section (don't tax the simple case).
  const sections = $derived<Section[]>(
    (() => {
      const list = $forwards;
      if ($groups.length === 0) {
        return list.length > 0
          ? [{ id: "__flat__", label: "", color: null, members: list }]
          : [];
      }
      const groupIds = new Set($groups.map((g) => g.id));
      const out: Section[] = [];
      for (const g of [...$groups].sort((a, b) => a.order - b.order)) {
        const members = list.filter((f) => f.groupId === g.id);
        if (members.length > 0) {
          out.push({ id: g.id, label: g.name, color: g.color, members });
        }
      }
      const ungrouped = list.filter(
        (f) => f.groupId === null || !groupIds.has(f.groupId),
      );
      if (ungrouped.length > 0) {
        out.push({
          id: "__ungrouped__",
          label: "Ungrouped",
          color: null,
          members: ungrouped,
        });
      }
      return out;
    })(),
  );

  const hasForwards = $derived($forwards.length > 0);
  const anyConnectable = $derived(
    $forwards.some((f) => {
      const s = $statusById[f.id] ?? "disconnected";
      return s === "disconnected" || s === "error";
    }),
  );

  function actionLabel(status: ForwardStatus, name: string): string {
    switch (status) {
      case "connected":
        return `Disconnect ${name}`;
      case "error":
        return `Retry ${name}`;
      case "connecting":
      case "disconnecting":
        return `${name} (busy)`;
      default:
        return `Connect ${name}`;
    }
  }

  async function primaryAction(f: ForwardConfig): Promise<void> {
    const status = $statusById[f.id] ?? "disconnected";
    try {
      if (status === "connected") await disconnectForward(f.id);
      else if (status === "error") await retryForward(f.id);
      else if (status === "disconnected") await connectForward(f.id);
      // connecting/disconnecting are transitional → ignore the click.
    } catch {
      // A failed intent surfaces via the backend status event (honest state);
      // the popover has no toast host, so we don't double-report here.
    }
  }

  function openSettings(): void {
    void showWindow();
    void hideTrayPopover();
  }

  // Arrow-key navigation across rows (keyboard-first, spec §1).
  function onListKeydown(e: KeyboardEvent): void {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    const rows = Array.from(
      listEl?.querySelectorAll<HTMLElement>("[data-tray-row]") ?? [],
    );
    if (rows.length === 0) return;
    const idx = rows.indexOf(document.activeElement as HTMLElement);
    e.preventDefault();
    const dir = e.key === "ArrowDown" ? 1 : -1;
    const next = idx === -1 ? 0 : (idx + dir + rows.length) % rows.length;
    rows[next].focus();
  }

  function onWindowKeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      void hideTrayPopover();
    }
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<div class="tray" role="dialog" aria-label="Tunnel Pilot tray menu">
  {#if updateAvailable}
    <button type="button" class="update" onclick={openSettings}>
      <Icon name="arrow-up" size={14} />
      <span>
        Update available{$updateStatus?.version
          ? ` · ${$updateStatus.version}`
          : ""}
      </span>
    </button>
  {/if}

  <div
    bind:this={listEl}
    class="scroll"
    onkeydown={onListKeydown}
    role="presentation"
  >
    {#if !hasForwards}
      <div class="empty">
        <Icon name="plug-zap" size={28} stroke={1.5} />
        <p>No tunnels yet</p>
        <span class="empty-sub">Add one from the main window.</span>
      </div>
    {:else}
      {#each sections as section (section.id)}
        {#if section.label}
          <div class="hd">
            {#if section.id !== "__ungrouped__"}
              <span
                class="hd-dot"
                style="background: {groupColorVar(section.color)}"
                aria-hidden="true"
              ></span>
            {/if}
            <span class="hd-label">{section.label}</span>
          </div>
        {/if}
        {#each section.members as f (f.id)}
          {@const status = $statusById[f.id] ?? "disconnected"}
          <button
            type="button"
            class="row"
            data-tray-row
            title={status === "error" && $lastErrorById[f.id]
              ? $lastErrorById[f.id]
              : undefined}
            aria-label={actionLabel(status, f.name)}
            onclick={() => void primaryAction(f)}
          >
            <StatusDot {status} />
            <span class="nm">{f.name}</span>
            <span class="rt mono">:{f.localPort}</span>
          </button>
        {/each}
      {/each}
    {/if}
  </div>

  <div class="sep" aria-hidden="true"></div>

  <footer class="foot">
    <button
      type="button"
      class="action"
      disabled={!anyConnectable}
      onclick={() => void startAll()}
    >
      <Icon name="play" size={14} /> Start all
    </button>
    <button type="button" class="action" onclick={openSettings}>
      <Icon name="settings" size={14} /> Settings
    </button>
    <button type="button" class="action" onclick={() => void quitApp()}>
      <Icon name="power" size={14} /> Quit
    </button>
  </footer>
</div>

<style>
  .tray {
    display: flex;
    flex-direction: column;
    height: 100%;
    width: 100%;
    background: var(--surface-overlay);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
    padding: var(--sp-3);
    overflow: hidden;
    color: var(--text);
  }

  .update {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    padding: var(--sp-3);
    margin-bottom: var(--sp-2);
    border: none;
    border-radius: var(--radius-sm);
    background: var(--accent-subtle);
    color: var(--accent-text);
    font-size: var(--fs-body-sm);
    font-weight: 600;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease-standard);
  }
  .update:hover {
    background: var(--accent-subtle-2);
  }
  .update span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    /* Trim the native scrollbar gutter so rows keep their rhythm. */
    margin-right: calc(-1 * var(--sp-1));
    padding-right: var(--sp-1);
  }

  .hd {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-3) var(--sp-2);
  }
  .hd-dot {
    flex: none;
    width: var(--sp-3);
    height: var(--sp-3);
    border-radius: var(--radius-full);
  }
  .hd-label {
    font-size: var(--fs-label);
    line-height: var(--lh-label);
    font-weight: var(--fw-label);
    letter-spacing: var(--tracking-label);
    text-transform: uppercase;
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: inherit;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease-standard);
  }
  .row:hover {
    background: var(--hover);
  }
  .row:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .nm {
    min-width: 0;
    flex: 1;
    font-size: var(--fs-body-sm);
    font-weight: 500;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rt {
    flex: none;
    font-size: var(--fs-mono-sm);
    color: var(--text-3);
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    height: 100%;
    padding: var(--sp-7) var(--sp-4);
    color: var(--text-3);
    text-align: center;
  }
  .empty p {
    margin: 0;
    font-size: var(--fs-body);
    font-weight: 500;
    color: var(--text-2);
  }
  .empty-sub {
    font-size: var(--fs-body-sm);
    color: var(--text-3);
  }

  .sep {
    height: var(--border-w);
    background: var(--divider);
    margin: var(--sp-2) 0;
  }

  .foot {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
  }
  .action {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex: 1;
    justify-content: center;
    padding: var(--sp-2) var(--sp-2);
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-2);
    font-size: var(--fs-body-sm);
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .action:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .action:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .action:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
