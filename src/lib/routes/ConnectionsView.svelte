<script lang="ts">
  import type { ForwardConfig, TunnelGroup } from "../types";
  import {
    forwards,
    statusById,
    statsById,
    lastErrorById,
    connectedCount,
  } from "../stores/forwards";
  import { settings } from "../stores/settings";
  import { groups } from "../stores/groups";
  import { activeTag } from "../stores/groups";
  import {
    deleteForward,
    duplicateForward,
    copySshCommand,
    deleteGroup,
  } from "../ipc";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import { activeView } from "../ui/view";
  import { paletteOpen } from "../stores/palette";
  import { pendingForm, pendingDelete } from "../stores/commands";
  import { pushToast } from "../ui/toast";
  import ConnectionList from "../components/ConnectionList.svelte";
  import TagFilterBar from "../components/TagFilterBar.svelte";
  import ForwardForm from "../components/ForwardForm.svelte";
  import ConfirmDialog from "../components/ConfirmDialog.svelte";
  import GroupFormDialog from "../components/GroupFormDialog.svelte";
  import Dialog from "../components/ui/Dialog.svelte";
  import Button from "../components/ui/Button.svelte";
  import Icon from "../components/ui/Icon.svelte";
  import Menu, { type MenuItem } from "../components/ui/Menu.svelte";
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
  let overflowOpen = $state(false);
  // Group management (Feature A): create/rename dialog + delete confirm.
  let groupForm = $state<{ mode: "add" | "edit"; group?: TunnelGroup } | null>(
    null,
  );
  let groupDeleteTarget = $state<TunnelGroup | null>(null);
  let deletingGroup = $state(false);

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
  const filterActive = $derived(q !== "" || $activeTag !== null);

  function matchesFilter(f: ForwardConfig): boolean {
    if ($activeTag !== null && !f.tags.includes($activeTag)) return false;
    if (q === "") return true;
    const hay =
      `${f.name} ${f.sshHost} ${f.localBindAddress}:${f.localPort} ${f.remoteHost}:${f.remotePort} ${f.tags.join(" ")}`.toLowerCase();
    return hay.includes(q);
  }

  // How many forwards survive the current filter — decides the empty state.
  const visibleCount = $derived($forwards.filter(matchesFilter).length);

  // Tags in use, with counts, for the toolbar tag filter (auto-pruned to 0).
  const tagCounts = $derived(
    (() => {
      const counts: Record<string, number> = {};
      for (const f of $forwards)
        for (const t of f.tags) counts[t] = (counts[t] ?? 0) + 1;
      return Object.entries(counts)
        .map(([name, count]) => ({ name, count }))
        .sort((a, b) => a.name.localeCompare(b.name));
    })(),
  );

  const selected = $derived($forwards.find((f) => f.id === selectedId) ?? null);
  const total = $derived($forwards.length);

  // Toolbar overflow (Compact breakpoint collapses New group / Duplicate /
  // Delete into ⋯).
  const overflowItems = $derived<MenuItem[]>([
    {
      label: "New group",
      icon: "folder",
      run: () => (groupForm = { mode: "add" }),
    },
    {
      label: "Duplicate",
      icon: "files",
      disabled: !selected,
      run: () => void duplicateSelected(),
    },
    {
      label: "Delete",
      icon: "trash",
      danger: true,
      disabled: !selected,
      run: () => (confirmTarget = selected),
    },
  ]);

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

  // Auto-prune a tag filter that no longer matches any tunnel (spec §4.2).
  $effect(() => {
    if ($activeTag !== null && !tagCounts.some((t) => t.name === $activeTag)) {
      activeTag.set(null);
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

  async function confirmDeleteGroup(): Promise<void> {
    if (!groupDeleteTarget) return;
    deletingGroup = true;
    try {
      // Backend clears groupId on member tunnels → they fall to Ungrouped;
      // deleting a group never deletes its tunnels (spec §4.2).
      await deleteGroup(groupDeleteTarget.id);
      pushToast("Group deleted", { tone: "success" });
      groupDeleteTarget = null;
    } catch (err) {
      pushToast(`Delete group failed: ${String(err)}`, { tone: "error" });
    } finally {
      deletingGroup = false;
    }
  }

  // Count members so the delete confirm can say where they'll go.
  const groupMemberCount = $derived(
    groupDeleteTarget
      ? $forwards.filter((f) => f.groupId === groupDeleteTarget!.id).length
      : 0,
  );

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
      {#if tagCounts.length > 0}
        <TagFilterBar
          tags={tagCounts}
          activeTag={$activeTag}
          onSelect={(t) => activeTag.set(t)}
        />
      {/if}
      <span class="wide-only">
        <Button
          variant="ghost"
          iconLeft="folder"
          title="Create a new group"
          onclick={() => (groupForm = { mode: "add" })}
        >
          New group
        </Button>
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
      </span>
      <div class="compact-only overflow">
        <Button
          variant="ghost"
          iconOnly="more-horizontal"
          ariaLabel="More actions"
          title="More actions"
          onclick={() => (overflowOpen = !overflowOpen)}
        />
        {#if overflowOpen}
          <Menu items={overflowItems} onClose={() => (overflowOpen = false)} />
        {/if}
      </div>
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
    <div class="col">
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
    {:else if filterActive && visibleCount === 0}
      <EmptyState
        icon="search"
        title="No matches"
        body={$activeTag !== null && q === ""
          ? `No tunnels tagged “${$activeTag}”.`
          : `No tunnels match “${filter.trim()}”.`}
      >
        {#snippet action()}
          <Button
            onclick={() => {
              filter = "";
              activeTag.set(null);
            }}
          >
            Clear filter
          </Button>
        {/snippet}
      </EmptyState>
    {:else}
      <ConnectionList
        forwards={$forwards}
        groups={$groups}
        statusById={$statusById}
        statsById={$statsById}
        lastErrorById={$lastErrorById}
        {selectedId}
        filterQuery={filter}
        activeTag={$activeTag}
        onSelect={(id) => (selectedId = id)}
        onEdit={(f) => (form = { mode: "edit", forward: f })}
        onDelete={(f) => (confirmTarget = f)}
        onViewLog={() => activeView.set("activity")}
        onEditGroup={(g) => (groupForm = { mode: "edit", group: g })}
        onDeleteGroup={(g) => (groupDeleteTarget = g)}
      />
    {/if}
    </div>
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

{#if groupForm}
  <GroupFormDialog
    mode={groupForm.mode}
    group={groupForm.group}
    onClose={() => (groupForm = null)}
  />
{/if}

{#if groupDeleteTarget}
  <Dialog title="Delete group?" size="sm" onClose={() => (groupDeleteTarget = null)}>
    <p class="dlg-body">
      <strong>{groupDeleteTarget.name}</strong> will be removed.
      {#if groupMemberCount > 0}
        Its {groupMemberCount}
        {groupMemberCount === 1 ? "tunnel" : "tunnels"} will move to Ungrouped —
        nothing is deleted.
      {:else}
        This group is empty.
      {/if}
    </p>
    {#snippet footer()}
      <Button onclick={() => (groupDeleteTarget = null)}>Cancel</Button>
      <Button
        variant="danger"
        loading={deletingGroup}
        onclick={() => void confirmDeleteGroup()}
      >
        Delete group
      </Button>
    {/snippet}
  </Dialog>
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
  /* Content column caps at 720 (scannable) and centers only when the content
     area is wide enough to leave symmetric whitespace (spec §3 Wide). */
  .col {
    max-width: 720px;
    margin-inline: 0;
  }
  .wide-only {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .compact-only {
    display: none;
  }
  .overflow {
    position: relative;
  }

  @container content (min-width: 1100px) {
    .col {
      margin-inline: auto;
    }
  }
  @container content (max-width: 640px) {
    .toolbar {
      padding: var(--sp-4) var(--sp-5);
    }
    .scroll {
      padding: var(--sp-4) var(--sp-5) var(--sp-7);
    }
    .filter-input {
      width: 120px;
    }
    .wide-only {
      display: none;
    }
    .compact-only {
      display: inline-flex;
    }
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
  .dlg-body {
    margin: 0;
    font-size: var(--fs-body);
    line-height: var(--lh-body);
    color: var(--text);
  }
</style>
