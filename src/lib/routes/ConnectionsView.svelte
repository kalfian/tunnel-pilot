<script lang="ts">
  import type { ForwardConfig } from "../types";
  import {
    forwards,
    statusById,
    statsById,
    lastErrorById,
    connectedCount,
  } from "../stores/forwards";
  import { settings } from "../stores/settings";
  import { deleteForward, duplicateForward, copySshCommand } from "../ipc";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { activeView } from "../ui/view";
  import { paletteOpen } from "../stores/palette";
  import { pendingForm, pendingDelete } from "../stores/commands";
  import { pushToast } from "../ui/toast";
  import ConnectionList from "../components/ConnectionList.svelte";
  import ForwardForm from "../components/ForwardForm.svelte";
  import ConfirmDialog from "../components/ConfirmDialog.svelte";
  import Button from "../components/ui/Button.svelte";
  import Icon from "../components/ui/Icon.svelte";
  import EmptyState from "../components/ui/EmptyState.svelte";
  import Skeleton from "../components/ui/Skeleton.svelte";

  let selectedId = $state<string | null>(null);
  let filter = $state("");
  let filterEl = $state<HTMLInputElement | undefined>();
  let form = $state<{ mode: "add" | "edit"; forward?: ForwardConfig } | null>(
    null,
  );
  let confirmTarget = $state<ForwardConfig | null>(null);
  let deleting = $state(false);

  // Skeleton only if the (local, fast) hydrate hasn't landed within 120ms.
  const hydrated = $derived($settings !== null);
  let showSkeleton = $state(false);
  $effect(() => {
    if (hydrated) {
      showSkeleton = false;
      return;
    }
    const t = setTimeout(() => (showSkeleton = true), 120);
    return () => clearTimeout(t);
  });

  const q = $derived(filter.trim().toLowerCase());
  const visible = $derived(
    q === ""
      ? $forwards
      : $forwards.filter((f) => {
          const hay =
            `${f.name} ${f.sshHost} ${f.localBindAddress}:${f.localPort} ${f.remoteHost}:${f.remotePort}`.toLowerCase();
          return hay.includes(q);
        }),
  );

  const selected = $derived($forwards.find((f) => f.id === selectedId) ?? null);
  const total = $derived($forwards.length);

  $effect(() => {
    // Drop a stale selection if the tunnel disappeared (deleted elsewhere).
    if (selectedId && !$forwards.some((f) => f.id === selectedId)) {
      selectedId = null;
    }
  });

  // Command-bus: the palette / global shortcuts publish add/edit/delete requests
  // (they can't reach this view's dialog state directly). Consume + clear them.
  $effect(() => {
    const req = $pendingForm;
    if (req) {
      form = req.mode === "add" ? { mode: "add" } : { mode: "edit", forward: req.forward };
      pendingForm.set(null);
    }
  });
  $effect(() => {
    const target = $pendingDelete;
    if (target) {
      confirmTarget = target;
      pendingDelete.set(null);
    }
  });

  async function duplicateSelected(): Promise<void> {
    if (!selected) return;
    try {
      await duplicateForward(selected.id);
      pushToast("Tunnel duplicated", { tone: "success" });
    } catch (err) {
      pushToast(`Duplicate failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function copySelected(): Promise<void> {
    if (!selected) return;
    try {
      const cmd = await copySshCommand(selected.id);
      await writeText(cmd);
      pushToast("SSH command copied", { tone: "success" });
    } catch (err) {
      pushToast(`Copy failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function confirmDelete(): Promise<void> {
    if (!confirmTarget) return;
    deleting = true;
    try {
      await deleteForward(confirmTarget.id);
      pushToast("Tunnel deleted", { tone: "success" });
      confirmTarget = null;
    } catch (err) {
      pushToast(`Delete failed: ${String(err)}`, { tone: "error" });
    } finally {
      deleting = false;
    }
  }

  function isTypingTarget(el: EventTarget | null): boolean {
    const node = el as HTMLElement | null;
    return (
      !!node &&
      (node.tagName === "INPUT" ||
        node.tagName === "TEXTAREA" ||
        node.tagName === "SELECT")
    );
  }

  function onKeydown(e: KeyboardEvent): void {
    if (form || confirmTarget || $paletteOpen) return; // dialogs/palette own the keyboard
    const mod = e.metaKey || e.ctrlKey;
    // ⌘N (add) is handled globally in App via the command bus.
    if (mod && e.key === "f") {
      e.preventDefault();
      filterEl?.focus();
      return;
    }
    if (isTypingTarget(e.target)) return;
    if (mod && e.key === "d" && selected) {
      e.preventDefault();
      void duplicateSelected();
    } else if (mod && e.key === "c" && selected) {
      e.preventDefault();
      void copySelected();
    } else if ((e.key === "Backspace" || e.key === "Delete") && selected) {
      e.preventDefault();
      confirmTarget = selected;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<section class="view">
  <header class="toolbar">
    <div class="titles">
      <h1 class="title">Connections</h1>
      <p class="subtitle">
        {total}
        {total === 1 ? "tunnel" : "tunnels"}
        {#if $connectedCount > 0}
          · <span class="active">{$connectedCount} active</span>
        {/if}
      </p>
    </div>
    <div class="tools">
      <div class="filter">
        <span class="filter-ic" aria-hidden="true"
          ><Icon name="search" size={15} /></span
        >
        <input
          bind:this={filterEl}
          class="filter-input"
          type="text"
          placeholder="Filter…"
          aria-label="Filter tunnels"
          bind:value={filter}
        />
      </div>
      <Button
        variant="ghost"
        iconOnly="files"
        ariaLabel="Duplicate selected tunnel"
        title="Duplicate"
        disabled={!selected}
        onclick={() => void duplicateSelected()}
      />
      <Button
        variant="ghost"
        iconOnly="trash"
        ariaLabel="Delete selected tunnel"
        title="Delete"
        disabled={!selected}
        onclick={() => (confirmTarget = selected)}
      />
      <Button
        variant="primary"
        iconLeft="plus"
        onclick={() => (form = { mode: "add" })}
      >
        Add
      </Button>
    </div>
  </header>

  <div class="scroll">
    {#if !hydrated && showSkeleton}
      <div class="skeletons">
        <Skeleton variant="card" />
        <Skeleton variant="card" />
        <Skeleton variant="card" />
      </div>
    {:else if total === 0}
      <EmptyState
        icon="plug-zap"
        title="No tunnels yet"
        body="Create your first SSH port forward to start tunneling traffic through a bastion."
      >
        {#snippet action()}
          <Button
            variant="primary"
            iconLeft="plus"
            onclick={() => (form = { mode: "add" })}
          >
            Add your first tunnel
          </Button>
          <button
            type="button"
            class="link"
            onclick={() => activeView.set("settings")}
          >
            Import from backup
          </button>
        {/snippet}
      </EmptyState>
    {:else if visible.length === 0}
      <EmptyState
        icon="search"
        title="No matches"
        body={`No tunnels match “${filter.trim()}”.`}
      >
        {#snippet action()}
          <Button onclick={() => (filter = "")}>Clear filter</Button>
        {/snippet}
      </EmptyState>
    {:else}
      <ConnectionList
        forwards={visible}
        statusById={$statusById}
        statsById={$statsById}
        lastErrorById={$lastErrorById}
        {selectedId}
        onSelect={(id) => (selectedId = id)}
        onEdit={(f) => (form = { mode: "edit", forward: f })}
        onDelete={(f) => (confirmTarget = f)}
        onViewLog={() => activeView.set("activity")}
      />
    {/if}
  </div>
</section>

{#if form}
  <ForwardForm
    mode={form.mode}
    forward={form.forward}
    onClose={() => (form = null)}
  />
{/if}

{#if confirmTarget}
  <ConfirmDialog
    name={confirmTarget.name}
    connected={($statusById[confirmTarget.id] ?? "disconnected") ===
      "connected"}
    busy={deleting}
    onConfirm={() => void confirmDelete()}
    onClose={() => (confirmTarget = null)}
  />
{/if}

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
  .titles {
    min-width: 0;
  }
  .title {
    margin: 0;
    font-size: var(--fs-title-lg);
    line-height: var(--lh-title-lg);
    font-weight: var(--fw-title-lg);
    letter-spacing: var(--tracking-tight);
    color: var(--text);
  }
  .subtitle {
    margin: var(--sp-1) 0 0;
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }
  .active {
    color: var(--status-connected-fg);
    font-weight: 500;
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
    width: 168px;
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
    padding: var(--sp-4) var(--sp-6) var(--sp-7);
  }
  .skeletons {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .link {
    border: none;
    background: transparent;
    color: var(--accent-text);
    font-size: var(--fs-body);
    cursor: pointer;
    padding: 0;
  }
  .link:hover {
    text-decoration: underline;
  }
</style>
