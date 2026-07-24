<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import type { ForwardConfig, ForwardStatus, TunnelStats } from "../types";
  import {
    connectForward,
    disconnectForward,
    retryForward,
    duplicateForward,
    copySshCommand,
  } from "../ipc";
  import { pushToast } from "../ui/toast";
  import { formatRoute } from "../ui/format";
  import StatusDot from "./ui/StatusDot.svelte";
  import Toggle from "./ui/Toggle.svelte";
  import StatChips from "./StatChips.svelte";
  import Menu, { type MenuItem } from "./ui/Menu.svelte";
  import Icon from "./ui/Icon.svelte";

  interface Props {
    forward: ForwardConfig;
    status: ForwardStatus;
    stats: TunnelStats;
    lastError: string | null;
    selected: boolean;
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
    onSelect,
    onEdit,
    onDelete,
    onViewLog,
    onNav,
    onReorder,
  }: Props = $props();

  let menuOpen = $state(false);

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

  const menuItems = $derived<MenuItem[]>([
    {
      label: "Copy SSH command",
      icon: "terminal",
      run: () => void copyCommand(),
    },
    { label: "Edit", icon: "pencil", run: onEdit },
    { label: "Duplicate", icon: "files", run: () => void duplicate() },
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
        if (e.altKey) {
          e.preventDefault();
          onReorder?.(1);
        } else {
          e.preventDefault();
          onNav?.(1);
        }
        break;
      case "ArrowUp":
        if (e.altKey) {
          e.preventDefault();
          onReorder?.(-1);
        } else {
          e.preventDefault();
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
  <span class="grip" aria-hidden="true"
    ><Icon name="grip-vertical" size={14} /></span
  >

  <button
    type="button"
    class="body"
    aria-pressed={selected}
    data-testid="row-body"
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
      </span>
      {#if status === "error"}
        <span class="route mono selectable">{route}</span>
      {:else if SUBTITLE[status]}
        <span class="subtitle">{SUBTITLE[status]}</span>
      {:else}
        <span class="route mono selectable">{route}</span>
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
    align-items: center;
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
    color: var(--text-3);
    opacity: 0;
    cursor: grab;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .card:hover .grip {
    opacity: 1;
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
    gap: var(--sp-2);
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
