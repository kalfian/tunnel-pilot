<script lang="ts">
  import type { ForwardConfig, ForwardStatus, TunnelStats } from "../types";
  import { reorderForwards } from "../ipc";
  import { EMPTY_STATS } from "../stores/forwards";
  import { pushToast } from "../ui/toast";
  import ConnectionRow from "./ConnectionRow.svelte";

  interface Props {
    forwards: ForwardConfig[];
    statusById: Record<string, ForwardStatus>;
    statsById: Record<string, TunnelStats>;
    lastErrorById: Record<string, string | null>;
    selectedId: string | null;
    onSelect: (id: string) => void;
    onEdit: (forward: ForwardConfig) => void;
    onDelete: (forward: ForwardConfig) => void;
    onViewLog: () => void;
  }

  const {
    forwards,
    statusById,
    statsById,
    lastErrorById,
    selectedId,
    onSelect,
    onEdit,
    onDelete,
    onViewLog,
  }: Props = $props();

  let listEl: HTMLUListElement | undefined = $state();
  let announce = $state("");

  // Optimistic display order (writable derived): drag/keyboard reassign it for
  // instant feedback, and it re-syncs whenever the backend confirms a new list
  // (spec §0/§4.5 — reorder is the one place the UI leads, then settles to Rust).
  let orderedIds = $derived(forwards.map((f) => f.id));

  const byId = $derived(new Map(forwards.map((f) => [f.id, f])));
  const ordered = $derived(
    orderedIds
      .map((id) => byId.get(id))
      .filter((f): f is ForwardConfig => f !== undefined),
  );

  let draggingId = $state<string | null>(null);

  function reorderTo(id: string, targetIndex: number): void {
    const from = orderedIds.indexOf(id);
    if (from === -1 || targetIndex < 0 || targetIndex >= orderedIds.length)
      return;
    const next = [...orderedIds];
    next.splice(from, 1);
    next.splice(targetIndex, 0, id);
    orderedIds = next;
  }

  async function persistOrder(): Promise<void> {
    try {
      await reorderForwards(orderedIds);
    } catch (err) {
      pushToast(`Reorder failed: ${String(err)}`, { tone: "error" });
    }
  }

  function focusRow(id: string): void {
    queueMicrotask(() => {
      const idx = orderedIds.indexOf(id);
      const el = listEl?.querySelectorAll<HTMLElement>(
        '[data-testid="row-body"]',
      )[idx];
      el?.focus();
    });
  }

  function navFrom(id: string, dir: -1 | 1): void {
    const idx = orderedIds.indexOf(id);
    const nextIdx = idx + dir;
    if (nextIdx < 0 || nextIdx >= orderedIds.length) return;
    const nextId = orderedIds[nextIdx];
    onSelect(nextId);
    focusRow(nextId);
  }

  function keyboardReorder(id: string, dir: -1 | 1): void {
    const idx = orderedIds.indexOf(id);
    reorderTo(id, idx + dir);
    const f = byId.get(id);
    const pos = orderedIds.indexOf(id) + 1;
    announce = `${f?.name ?? "Tunnel"} moved to position ${pos} of ${orderedIds.length}`;
    focusRow(id);
    void persistOrder();
  }
</script>

<ul bind:this={listEl} class="list" aria-label="Tunnels">
  {#each ordered as forward (forward.id)}
    <li
      class="row"
      class:dragging={draggingId === forward.id}
      draggable="true"
      ondragstart={() => (draggingId = forward.id)}
      ondragend={() => {
        draggingId = null;
        void persistOrder();
      }}
      ondragover={(e) => {
        e.preventDefault();
        if (draggingId && draggingId !== forward.id) {
          reorderTo(draggingId, orderedIds.indexOf(forward.id));
        }
      }}
    >
      <ConnectionRow
        {forward}
        status={statusById[forward.id] ?? "disconnected"}
        stats={statsById[forward.id] ?? EMPTY_STATS}
        lastError={lastErrorById[forward.id] ?? null}
        selected={selectedId === forward.id}
        onSelect={() => onSelect(forward.id)}
        onEdit={() => onEdit(forward)}
        onDelete={() => onDelete(forward)}
        {onViewLog}
        onNav={(dir) => navFrom(forward.id, dir)}
        onReorder={(dir) => keyboardReorder(forward.id, dir)}
      />
    </li>
  {/each}
</ul>

<div class="sr-only" role="status" aria-live="polite">{announce}</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    transition:
      transform var(--dur-reorder) var(--ease-standard),
      opacity var(--dur-fast) var(--ease-standard);
  }
  .row.dragging {
    opacity: 0.6;
    transform: scale(1.02);
  }
  .row.dragging :global(.card) {
    box-shadow: var(--shadow-2);
    border-color: var(--border-strong);
  }
  @media (prefers-reduced-motion: reduce) {
    .row {
      transition: none;
    }
    .row.dragging {
      transform: none;
    }
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>
