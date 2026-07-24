<script lang="ts">
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import type { LogEntry, LogLevel } from "../types";
  import { logs } from "../stores/logs";
  import { clearLogs, getLogsText } from "../ipc";
  import { pushToast } from "../ui/toast";
  import Button from "../components/ui/Button.svelte";
  import Select from "../components/ui/Select.svelte";
  import Icon from "../components/ui/Icon.svelte";
  import EmptyState from "../components/ui/EmptyState.svelte";

  const RANK: Record<LogLevel, number> = { info: 0, warning: 1, error: 2 };
  const LEVEL_LABEL: Record<LogLevel, string> = {
    info: "INFO",
    warning: "WARN",
    error: "ERROR",
  };

  let threshold = $state(-1); // -1 = all
  let filter = $state("");

  const q = $derived(filter.trim().toLowerCase());
  const visible = $derived(
    $logs.filter((l) => {
      if (RANK[l.level] < threshold) return false;
      if (q === "") return true;
      return (
        l.message.toLowerCase().includes(q) ||
        (l.tunnelName ?? "").toLowerCase().includes(q)
      );
    }),
  );

  function line(l: LogEntry): string {
    const tunnel = l.tunnelName ? ` [${l.tunnelName}]` : "";
    return `[${l.timestamp}] [${LEVEL_LABEL[l.level]}]${tunnel} ${l.message}`;
  }

  async function copyRow(l: LogEntry): Promise<void> {
    try {
      await writeText(line(l));
      pushToast("Log line copied", { tone: "success" });
    } catch (err) {
      pushToast(`Copy failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function copyAll(): Promise<void> {
    try {
      const text = await getLogsText();
      await writeText(text);
      pushToast("All logs copied", { tone: "success" });
    } catch (err) {
      pushToast(`Copy failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function clear(): Promise<void> {
    try {
      await clearLogs();
      pushToast("Logs cleared", { tone: "info" });
    } catch (err) {
      pushToast(`Clear failed: ${String(err)}`, { tone: "error" });
    }
  }
</script>

<section class="view">
  <header class="toolbar">
    <div class="titles">
      <h1 class="title">Activity</h1>
      <p class="subtitle">
        {$logs.length}
        {$logs.length === 1 ? "entry" : "entries"}
      </p>
    </div>
    <div class="tools">
      <Select
        ariaLabel="Filter by level"
        value={threshold}
        options={[
          { value: -1, label: "All levels" },
          { value: 0, label: "Info +" },
          { value: 1, label: "Warn +" },
          { value: 2, label: "Error" },
        ]}
        onchange={(v) => (threshold = v)}
      />
      <div class="filter">
        <span class="filter-ic" aria-hidden="true"
          ><Icon name="search" size={15} /></span
        >
        <input
          class="filter-input"
          type="text"
          placeholder="Filter…"
          aria-label="Filter log lines"
          bind:value={filter}
        />
      </div>
      <Button
        variant="ghost"
        iconLeft="copy"
        disabled={$logs.length === 0}
        onclick={() => void copyAll()}
      >
        Copy all
      </Button>
      <Button
        variant="ghost"
        iconLeft="trash"
        disabled={$logs.length === 0}
        onclick={() => void clear()}
      >
        Clear
      </Button>
    </div>
  </header>

  <div class="scroll">
    {#if $logs.length === 0}
      <EmptyState
        icon="scroll-text"
        title="No activity yet"
        body="Logs appear here when tunnels connect, reconnect, or report an error."
      />
    {:else if visible.length === 0}
      <EmptyState
        icon="search"
        title="No matching lines"
        body="No log entries match the current filter."
      />
    {:else}
      <ul class="log" aria-label="Activity log">
        {#each visible as l, i (i)}
          <li>
            <button
              type="button"
              class="log-row"
              title="Copy this line"
              onclick={() => void copyRow(l)}
            >
              <span class="ts mono">{l.timestamp}</span>
              <span class="lvl mono {l.level}">{LEVEL_LABEL[l.level]}</span>
              <span class="tunnel mono">{l.tunnelName ?? "—"}</span>
              <span class="msg mono">{l.message}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</section>

<style>
  .view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-width: 0;
  }
  .toolbar {
    position: sticky;
    top: 0;
    z-index: var(--z-sticky);
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-4);
    padding: var(--sp-5) var(--sp-6);
    background: var(--bg);
    border-bottom: var(--border-w) solid var(--divider);
  }
  .title {
    margin: 0;
    font-size: var(--fs-title-lg);
    line-height: var(--lh-title-lg);
    font-weight: var(--fw-title-lg);
    letter-spacing: var(--tracking-tight);
  }
  .subtitle {
    margin: var(--sp-1) 0 0;
    font-size: var(--fs-body-sm);
    color: var(--text-2);
  }
  .tools {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex: none;
  }
  .filter {
    position: relative;
    display: flex;
    align-items: center;
  }
  .filter-ic {
    position: absolute;
    left: var(--sp-3);
    display: grid;
    place-items: center;
    color: var(--text-3);
    pointer-events: none;
  }
  .filter-input {
    width: 148px;
    height: var(--btn-h);
    padding: 0 var(--sp-3) 0 var(--sp-8);
    border-radius: var(--radius-sm);
    border: var(--border-w) solid var(--border);
    background: var(--surface);
    color: var(--text);
    font-size: var(--fs-body);
  }
  :global([data-theme="dark"]) .filter-input {
    background: var(--surface-2);
  }
  .filter-input::placeholder {
    color: var(--text-3);
  }
  .filter-input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus-ring-halo);
  }
  .scroll {
    flex: 1;
    overflow-y: auto;
    padding: var(--sp-3) var(--sp-5) var(--sp-6);
  }
  .log {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .log-row {
    display: grid;
    grid-template-columns: 68px 52px 128px 1fr;
    gap: var(--sp-3);
    width: 100%;
    min-height: var(--log-row-h);
    padding: var(--sp-1) var(--sp-3);
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    text-align: left;
    cursor: pointer;
    font-size: var(--fs-mono-sm);
    line-height: var(--lh-mono-sm);
  }
  .log-row:hover {
    background: var(--hover);
  }
  .log-row:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .ts {
    color: var(--text-3);
  }
  .lvl {
    font-weight: 600;
  }
  .lvl.info {
    color: var(--text-2);
  }
  .lvl.warning {
    color: var(--status-pending-fg);
  }
  .lvl.error {
    color: var(--status-error-fg);
  }
  .tunnel {
    color: var(--accent-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .msg {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
