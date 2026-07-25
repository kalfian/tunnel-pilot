<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import type {
    ForwardConfig,
    ForwardStatus,
    TunnelGroup,
    TunnelStats,
  } from "../types";
  import {
    connectForward,
    disconnectForward,
    retryForward,
    duplicateForward,
    copySshCommand,
    assignForwardGroup,
  } from "../ipc";
  import { pushToast } from "../ui/toast";
  import { formatRoute } from "../ui/format";
  import StatusDot from "./ui/StatusDot.svelte";
  import Toggle from "./ui/Toggle.svelte";
  import StatChips from "./StatChips.svelte";
  import TagPill from "./ui/TagPill.svelte";
  import Menu, { type MenuItem } from "./ui/Menu.svelte";
  import Icon from "./ui/Icon.svelte";

  interface Props {
    forward: ForwardConfig;
    status: ForwardStatus;
    stats: TunnelStats;
    lastError: string | null;
    selected: boolean;
    /** All groups — powers the "Assign group ▸" menu. Empty = no group UI. */
    groups?: TunnelGroup[];
    /** When false, the drag grip + ⌥↑/↓ reorder are suppressed (e.g. while a
        filter is active — reorder would corrupt the persisted order, F43). */
    reorderable?: boolean;
    onSelect: () => void;
    onEdit: () => void;
    onDelete: () => void;
    onViewLog: () => void;
    /** Arrow-key selection move (delegated to the list). */
    onNav?: (dir: -1 | 1) => void;
    /** Alt+Arrow keyboard reorder (delegated to the list). */
    onReorder?: (dir: -1 | 1) => void;
  }

  const {
    forward,
    status,
    stats,
    lastError,
    selected,
    groups = [],
    reorderable = true,
    onSelect,
    onEdit,
    onDelete,
    onViewLog,
    onNav,
    onReorder,
  }: Props = $props();

  let menuOpen = $state(false);

  // Show at most 3 tag pills; overflow collapses into a "+N" pill.
  const shownTags = $derived(forward.tags.slice(0, 3));
  const extraTags = $derived(Math.max(0, forward.tags.length - 3));

  const pending = $derived(
    status === "connecting" || status === "disconnecting",
  );
  const isOn = $derived(status === "connected" || status === "connecting");
  const route = $derived(
    formatRoute(
      forward.localBindAddress,
      forward.localPort,
      forward.remoteHost,
      forward.remotePort,
    ),
  );

  const SUBTITLE: Partial<Record<ForwardStatus, string>> = {
    connecting: "Connecting…",
    disconnecting: "Disconnecting…",
  };

  async function toggle(next: boolean): Promise<void> {
    try {
      if (next) await connectForward(forward.id);
      else await disconnectForward(forward.id);
    } catch (err) {
      pushToast(`${next ? "Connect" : "Disconnect"} failed: ${String(err)}`, {
        tone: "error",
      });
    }
  }

  async function retry(): Promise<void> {
    try {
      await retryForward(forward.id);
    } catch (err) {
      pushToast(`Retry failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function duplicate(): Promise<void> {
    try {
      await duplicateForward(forward.id);
      pushToast("Tunnel duplicated", { tone: "success" });
    } catch (err) {
      pushToast(`Duplicate failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function copyCommand(): Promise<void> {
    try {
      const cmd = await copySshCommand(forward.id);
      await writeText(cmd);
      pushToast("SSH command copied", { tone: "success" });
    } catch (err) {
      pushToast(`Copy failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function assignGroup(groupId: string | null): Promise<void> {
    if ((forward.groupId ?? null) === groupId) return; // no-op
    try {
      await assignForwardGroup(forward.id, groupId);
    } catch (err) {
      pushToast(`Move to group failed: ${String(err)}`, { tone: "error" });
    }
  }

  // "Assign group ▸": a check marks the current group; picking another moves it.
  const assignSubmenu = $derived<MenuItem[]>([
    {
      label: "Ungrouped",
      icon: (forward.groupId ?? null) === null ? "check" : undefined,
      run: () => void assignGroup(null),
    },
    ...groups.map(
      (g): MenuItem => ({
        label: g.name,
        icon: forward.groupId === g.id ? "check" : "folder",
        run: () => void assignGroup(g.id),
      }),
    ),
  ]);

  const menuItems = $derived<MenuItem[]>([
    {
      label: "Copy SSH command",
      icon: "terminal",
      run: () => void copyCommand(),
    },
    { label: "Edit", icon: "pencil", run: onEdit },
    { label: "Duplicate", icon: "files", run: () => void duplicate() },
    { label: "Assign group", icon: "folder", submenu: assignSubmenu },
    { label: "Delete", icon: "trash", danger: true, run: onDelete },
  ]);

  function onBodyKeydown(e: KeyboardEvent): void {
    switch (e.key) {
      case " ":
        e.preventDefault();
        if (!pending) void toggle(!isOn);
        break;
      case "Enter":
        e.preventDefault();
        onEdit();
        break;
      case "Backspace":
      case "Delete":
        e.preventDefault();
        onDelete();
        break;
      case "ArrowDown":
        e.preventDefault();
        if (e.altKey) {
          if (reorderable) onReorder?.(1);
        } else {
          onNav?.(1);
        }
        break;
      case "ArrowUp":
        e.preventDefault();
        if (e.altKey) {
          if (reorderable) onReorder?.(-1);
        } else {
          onNav?.(-1);
        }
        break;
    }
  }
</script>

<div
  class="card"
  class:selected
  class:connected={status === "connected"}
  class:error={status === "error"}
>
  {#if reorderable}
    <span class="grip" aria-hidden="true"
      ><Icon name="grip-vertical" size={14} /></span
    >
  {:else}
    <span class="grip-spacer" aria-hidden="true"></span>
  {/if}

  <button
    type="button"
    class="body"
    aria-pressed={selected}
    data-testid="row-body"
    data-row-id={forward.id}
    onclick={onSelect}
    onkeydown={onBodyKeydown}
    oncontextmenu={(e) => {
      e.preventDefault();
      onSelect();
      menuOpen = true;
    }}
  >
    <span class="dot"><StatusDot {status} /></span>
    <span class="info">
      <span class="name-line">
        <span class="name">{forward.name}</span>
        {#if shownTags.length > 0}
          <span class="tags">
            {#each shownTags as tag (tag)}
              <TagPill label={tag} />
            {/each}
            {#if extraTags > 0}
              <TagPill label={`+${extraTags}`} />
            {/if}
          </span>
        {/if}
      </span>
      {#if status === "error"}
        <span class="route mono selectable" title={route}>{route}</span>
      {:else if SUBTITLE[status]}
        <span class="subtitle">{SUBTITLE[status]}</span>
      {:else}
        <span class="route mono selectable" title={route}>{route}</span>
      {/if}
      {#if status === "connected"}
        <StatChips {stats} />
      {/if}
    </span>
  </button>

  <div class="actions">
    <div class="menu-wrap">
      <button
        type="button"
        class="icon-btn"
        aria-label="Tunnel actions"
        aria-haspopup="menu"
        aria-expanded={menuOpen}
        onclick={() => (menuOpen = !menuOpen)}
      >
        <Icon name="more-horizontal" size={16} />
      </button>
      {#if menuOpen}
        <Menu items={menuItems} onClose={() => (menuOpen = false)} />
      {/if}
    </div>
    <Toggle
      checked={isOn}
      {pending}
      ariaLabel="{isOn ? 'Disconnect' : 'Connect'} {forward.name}"
      onchange={(next) => void toggle(next)}
    />
  </div>

  {#if status === "error"}
    <div class="err-strip" role="alert">
      <Icon name="alert-triangle" size={14} />
      <span class="err-msg">{lastError ?? "Connection failed"}</span>
      <button type="button" class="err-action" onclick={() => void retry()}>
        Retry
      </button>
      <button type="button" class="err-action" onclick={onViewLog}>
        View log
      </button>
    </div>
  {/if}
</div>

<style>
  .card {
    position: relative;
    display: grid;
    grid-template-columns: auto 1fr auto;
    /* Top-align so a 2-line and a 3-line (stats) row share one top rhythm and
       the dot / name / toggle land on the same line whatever the card height. */
    align-items: start;
    gap: var(--sp-2);
    min-height: var(--row-h);
    padding: var(--sp-3) var(--sp-4) var(--sp-3) var(--sp-2);
    background: var(--surface);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-md);
    transition:
      background-color var(--dur-fast) var(--ease-standard),
      border-color var(--dur-fast) var(--ease-standard),
      box-shadow var(--dur-fast) var(--ease-standard);
  }
  .card:hover {
    background: var(--hover);
  }
  .card.selected {
    background: var(--accent-subtle);
    border-color: var(--border-strong);
  }
  /* Ambient "this is live" accent left-rail. */
  .card.connected::before,
  .card.selected::before {
    content: "";
    position: absolute;
    left: 0;
    top: var(--sp-3);
    bottom: var(--sp-3);
    width: var(--border-w-emph);
    border-radius: var(--radius-full);
    background: var(--accent);
  }
  .card.error::before {
    content: "";
    position: absolute;
    left: 0;
    top: var(--sp-3);
    bottom: var(--sp-3);
    width: var(--border-w-emph);
    border-radius: var(--radius-full);
    background: var(--status-error);
  }

  .grip {
    display: flex;
    align-items: center;
    /* Centre the grip on the name line (matches the dot + toggle). */
    height: var(--lh-title-sm);
    color: var(--text-3);
    opacity: 0;
    cursor: grab;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .card:hover .grip {
    opacity: 1;
  }
  /* Keep the grid column stable when reorder is suppressed (filter active). */
  .grip-spacer {
    width: 14px;
  }

  .tags {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    overflow: hidden;
  }

  .body {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-3);
    min-width: 0;
    padding: 0;
    border: none;
    background: transparent;
    text-align: left;
    cursor: default;
    color: inherit;
  }
  .body:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
    border-radius: var(--radius-sm);
  }
  .dot {
    display: flex;
    align-items: center;
    height: var(--lh-title-sm);
  }
  .info {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-width: 0;
  }
  .name-line {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .name {
    min-width: 0;
    font-size: var(--fs-title-sm);
    line-height: var(--lh-title-sm);
    font-weight: var(--fw-title-sm);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .route {
    font-size: var(--fs-mono);
    line-height: var(--lh-mono);
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .subtitle {
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--status-pending-fg);
  }

  .actions {
    display: flex;
    align-items: center;
    /* Pin controls to the name line; overflow (28px hit target) stays centred
       on it so the toggle never drifts to the middle of a tall connected row. */
    height: var(--lh-title-sm);
    gap: var(--sp-3);
  }
  .menu-wrap {
    position: relative;
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .card:hover .menu-wrap,
  .card.selected .menu-wrap,
  .menu-wrap:focus-within {
    opacity: 1;
  }
  .icon-btn {
    display: grid;
    place-items: center;
    width: var(--hit-min);
    height: var(--hit-min);
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-2);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease-standard);
  }
  .icon-btn:hover {
    background: var(--hover);
    color: var(--text);
  }

  .err-strip {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-top: var(--sp-1);
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-sm);
    background: var(--status-error-bg);
    color: var(--status-error-fg);
    font-size: var(--fs-body-sm);
  }
  .err-msg {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .err-action {
    flex: none;
    border: none;
    background: transparent;
    color: var(--status-error-fg);
    font-weight: 600;
    font-size: var(--fs-body-sm);
    cursor: pointer;
    padding: 0 var(--sp-1);
  }
  .err-action:hover {
    text-decoration: underline;
  }
</style>
