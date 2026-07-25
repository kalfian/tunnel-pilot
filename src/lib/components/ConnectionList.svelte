<script lang="ts">
  import type {
    ForwardConfig,
    ForwardStatus,
    TunnelGroup,
    TunnelStats,
  } from "../types";
  import {
    reorderForwards,
    updateGroup,
    assignForwardGroup,
    startGroup,
    stopGroup,
  } from "../ipc";
  import { EMPTY_STATS } from "../stores/forwards";
  import { pushToast } from "../ui/toast";
  import ConnectionRow from "./ConnectionRow.svelte";
  import GroupHeader from "./GroupHeader.svelte";

  interface Props {
    /** FULL ordered forward list (unfiltered — filtering happens here so
        reorder always operates on the complete order; F43). */
    forwards: ForwardConfig[];
    groups: TunnelGroup[];
    statusById: Record<string, ForwardStatus>;
    statsById: Record<string, TunnelStats>;
    lastErrorById: Record<string, string | null>;
    selectedId: string | null;
    /** In-view search query (empty = no search filter). */
    filterQuery: string;
    /** Active tag filter (null = all). */
    activeTag: string | null;
    onSelect: (id: string) => void;
    onEdit: (forward: ForwardConfig) => void;
    onDelete: (forward: ForwardConfig) => void;
    onViewLog: () => void;
    /** Rename/recolor a group (opens the group form). */
    onEditGroup: (group: TunnelGroup) => void;
    /** Delete a group (tunnels fall back to Ungrouped). */
    onDeleteGroup: (group: TunnelGroup) => void;
  }

  const {
    forwards,
    groups,
    statusById,
    statsById,
    lastErrorById,
    selectedId,
    filterQuery,
    activeTag,
    onSelect,
    onEdit,
    onDelete,
    onViewLog,
    onEditGroup,
    onDeleteGroup,
  }: Props = $props();

  let listEl: HTMLDivElement | undefined = $state();
  let announce = $state("");

  const hasGroups = $derived(groups.length > 0);
  const groupIds = $derived(new Set(groups.map((g) => g.id)));
  const filterActive = $derived(filterQuery.trim() !== "" || activeTag !== null);
  // Reorder is the one optimistic interaction; disabled while filtering because
  // the visible subset doesn't map to adjacent full-order slots (F43).
  const reorderable = $derived(!filterActive);

  // Optimistic display order (writable derived): drag/keyboard reassign it for
  // instant feedback; it re-syncs when the backend confirms a new list.
  let orderedIds = $derived(forwards.map((f) => f.id));
  const byId = $derived(new Map(forwards.map((f) => [f.id, f])));
  const orderedForwards = $derived(
    orderedIds
      .map((id) => byId.get(id))
      .filter((f): f is ForwardConfig => f !== undefined),
  );

  function effectiveGroup(f: ForwardConfig | undefined): string | null {
    return f && f.groupId && groupIds.has(f.groupId) ? f.groupId : null;
  }

  function matches(f: ForwardConfig): boolean {
    if (activeTag !== null && !f.tags.includes(activeTag)) return false;
    const q = filterQuery.trim().toLowerCase();
    if (q === "") return true;
    const hay =
      `${f.name} ${f.sshHost} ${f.localBindAddress}:${f.localPort} ${f.remoteHost}:${f.remotePort} ${f.tags.join(" ")}`.toLowerCase();
    return hay.includes(q);
  }

  // Ungrouped section collapse is UI-local (no group model to persist to).
  let ungroupedCollapsed = $state(false);

  interface Section {
    group: TunnelGroup | null;
    /** Members after filtering (what renders). */
    visible: ForwardConfig[];
    /** All members regardless of filter (for the X/Y count). */
    activeCount: number;
    total: number;
    collapsed: boolean;
  }

  const sections = $derived<Section[]>(
    (() => {
      if (!hasGroups) {
        return [
          {
            group: null,
            visible: orderedForwards.filter(matches),
            activeCount: 0,
            total: orderedForwards.length,
            collapsed: false,
          },
        ];
      }
      const out: Section[] = [];
      const sortedGroups = [...groups].sort((a, b) => a.order - b.order);
      for (const g of sortedGroups) {
        const members = orderedForwards.filter(
          (f) => effectiveGroup(f) === g.id,
        );
        const visible = members.filter(matches);
        if (filterActive && visible.length === 0) continue;
        out.push({
          group: g,
          visible: g.collapsed ? [] : visible,
          activeCount: members.filter(
            (f) => (statusById[f.id] ?? "disconnected") === "connected",
          ).length,
          total: members.length,
          collapsed: g.collapsed,
        });
      }
      const ungrouped = orderedForwards.filter(
        (f) => effectiveGroup(f) === null,
      );
      const ungroupedVisible = ungrouped.filter(matches);
      if (ungrouped.length > 0 && !(filterActive && ungroupedVisible.length === 0)) {
        out.push({
          group: null,
          visible: ungroupedCollapsed ? [] : ungroupedVisible,
          activeCount: ungrouped.filter(
            (f) => (statusById[f.id] ?? "disconnected") === "connected",
          ).length,
          total: ungrouped.length,
          collapsed: ungroupedCollapsed,
        });
      }
      return out;
    })(),
  );

  // Flattened visible ids in render order — drives arrow-key navigation.
  const visibleIds = $derived(
    sections.flatMap((s) => s.visible.map((f) => f.id)),
  );

  // DnD model (two disjoint gestures, disambiguated by drop target):
  //  • REORDER — hover a row in the SAME group → live-splice `orderedIds`,
  //    committed on `dragend` via `reorderForwards` (the one optimistic path).
  //  • REASSIGN — drop onto a DIFFERENT group's section (header / body / gap)
  //    → `assignForwardGroup`, committed on the explicit `drop` (never on
  //    `dragend`, so releasing outside the list can't silently move a tunnel).
  //  Both are gated by `reorderable` (disabled under a filter — F43) via the
  //  row's `draggable` attribute, so neither gesture starts while filtering.
  let draggingId = $state<string | null>(null);
  let dragOverGroup = $state<string | null>(null);
  let dragStartOrder = "";

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

  // Highlight a section only when a drop there would actually REASSIGN — i.e.
  // dragging over a different group than the tunnel's current one. No highlight
  // while reordering within the same group.
  function isDropTarget(gid: string | null): boolean {
    if (draggingId === null || !hasGroups) return false;
    if (gid !== dragOverGroup) return false;
    return effectiveGroup(byId.get(draggingId)) !== gid;
  }

  // Row hover during a drag: same group → live reorder; different group → just
  // mark the reassign target (the section's `drop` commits it).
  function onRowDragOver(forward: ForwardConfig): void {
    if (!draggingId) return;
    const draggedGroup = effectiveGroup(byId.get(draggingId));
    const rowGroup = effectiveGroup(forward);
    dragOverGroup = rowGroup;
    if (draggedGroup === rowGroup && draggingId !== forward.id) {
      reorderTo(draggingId, orderedIds.indexOf(forward.id));
    }
  }

  // Explicit drop onto a section → reassign to that group (or Ungrouped=null).
  // Same-section drops are no-ops here (the reorder path already handled them).
  async function dropOnSection(targetGroupId: string | null): Promise<void> {
    const dragged = draggingId;
    if (!dragged || !hasGroups) return;
    const f = byId.get(dragged);
    if (f === undefined || effectiveGroup(f) === targetGroupId) return;
    try {
      await assignForwardGroup(dragged, targetGroupId);
    } catch (err) {
      pushToast(`Move to group failed: ${String(err)}`, { tone: "error" });
    }
  }

  async function onDragEnd(): Promise<void> {
    draggingId = null;
    dragOverGroup = null;
    // Commit a REORDER (orderedIds mutated live during dragover). A cross-group
    // move is committed on `drop`, not here, and leaves the order untouched — so
    // a no-op drag (no move) stays a guarded no-op.
    if (orderedIds.join(",") !== dragStartOrder) {
      await persistOrder();
    }
  }

  function focusRow(id: string): void {
    queueMicrotask(() => {
      listEl
        ?.querySelector<HTMLElement>(`[data-row-id="${CSS.escape(id)}"]`)
        ?.focus();
    });
  }

  function navFrom(id: string, dir: -1 | 1): void {
    const idx = visibleIds.indexOf(id);
    const nextIdx = idx + dir;
    if (nextIdx < 0 || nextIdx >= visibleIds.length) return;
    const nextId = visibleIds[nextIdx];
    onSelect(nextId);
    focusRow(nextId);
  }

  // Keyboard reorder: swap with the adjacent sibling in the SAME group so the
  // move stays within the group and never corrupts other groups' order.
  function keyboardReorder(id: string, dir: -1 | 1): void {
    if (!reorderable) {
      announce = "Reordering is disabled while a filter is active.";
      return;
    }
    const f = byId.get(id);
    const gid = effectiveGroup(f);
    const siblings = orderedIds.filter((x) => effectiveGroup(byId.get(x)) === gid);
    const pos = siblings.indexOf(id);
    const target = pos + dir;
    if (target < 0 || target >= siblings.length) return;
    const targetId = siblings[target];
    const i = orderedIds.indexOf(id);
    const j = orderedIds.indexOf(targetId);
    const next = [...orderedIds];
    [next[i], next[j]] = [next[j], next[i]];
    orderedIds = next;
    announce = `${f?.name ?? "Tunnel"} moved to position ${target + 1} of ${siblings.length} in its group`;
    focusRow(id);
    void persistOrder();
  }

  function toggleCollapse(section: Section): void {
    if (section.group) {
      const g = section.group;
      void updateGroup(g.id, {
        name: g.name,
        color: g.color,
        collapsed: !g.collapsed,
      }).catch((err) =>
        pushToast(`Couldn't update group: ${String(err)}`, { tone: "error" }),
      );
    } else {
      ungroupedCollapsed = !ungroupedCollapsed;
    }
  }
</script>

<div bind:this={listEl} class="wrap">
  {#each sections as section (section.group?.id ?? "__ungrouped__")}
    {@const gid = section.group?.id ?? null}
    <!-- Section wraps header + body so a drag can be dropped anywhere in the
         group (incl. an empty/collapsed group's header) to reassign. Pointer-
         only affordance; the keyboard path is the row ⋯ "Assign group" menu. -->
    <div
      class="section"
      class:drop-target={isDropTarget(gid)}
      data-section={gid ?? "__ungrouped__"}
      role="group"
      aria-label={section.group?.name ?? "Ungrouped"}
      ondragover={(e) => {
        if (!reorderable || !draggingId) return;
        e.preventDefault();
        dragOverGroup = gid;
      }}
      ondrop={(e) => {
        if (!reorderable || !draggingId) return;
        e.preventDefault();
        void dropOnSection(gid);
      }}
    >
    {#if hasGroups}
      <GroupHeader
        name={section.group?.name ?? "Ungrouped"}
        activeCount={section.activeCount}
        total={section.total}
        collapsed={section.collapsed}
        color={section.group?.color ?? null}
        onToggle={() => toggleCollapse(section)}
        onStartAll={() =>
          section.group &&
          void startGroup(section.group.id).catch((err) =>
            pushToast(`Start all failed: ${String(err)}`, { tone: "error" }),
          )}
        onStopAll={() =>
          section.group &&
          void stopGroup(section.group.id).catch((err) =>
            pushToast(`Stop all failed: ${String(err)}`, { tone: "error" }),
          )}
        showBulk={section.group !== null}
        onEdit={section.group ? () => onEditGroup(section.group!) : undefined}
        onDelete={section.group
          ? () => onDeleteGroup(section.group!)
          : undefined}
      />
    {/if}

    {#if section.visible.length > 0}
      <ul class="list" aria-label={section.group?.name ?? "Tunnels"}>
        {#each section.visible as forward (forward.id)}
          <li
            class="row"
            class:dragging={draggingId === forward.id}
            draggable={reorderable}
            ondragstart={() => {
              if (!reorderable) return;
              draggingId = forward.id;
              dragOverGroup = effectiveGroup(forward);
              dragStartOrder = orderedIds.join(",");
            }}
            ondragend={() => void onDragEnd()}
            ondragover={(e) => {
              if (!reorderable || !draggingId) return;
              e.preventDefault();
              onRowDragOver(forward);
            }}
          >
            <ConnectionRow
              {forward}
              {groups}
              status={statusById[forward.id] ?? "disconnected"}
              stats={statsById[forward.id] ?? EMPTY_STATS}
              lastError={lastErrorById[forward.id] ?? null}
              selected={selectedId === forward.id}
              {reorderable}
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
    {/if}
    </div>
  {/each}
</div>

<div class="sr-only" role="status" aria-live="polite">{announce}</div>

<style>
  .wrap {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    border-radius: var(--radius-md);
    /* Outline (not border/padding) so the drop highlight never reflows the
       list. Only the tint transitions — the ring is a state cue, not motion. */
    outline: var(--border-w-emph) solid transparent;
    outline-offset: var(--sp-1);
    transition: background-color var(--dur-fast) var(--ease-standard);
  }
  .section.drop-target {
    background: var(--accent-subtle);
    outline-color: var(--accent);
  }
  @media (prefers-reduced-motion: reduce) {
    .section {
      transition: none;
    }
  }
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
