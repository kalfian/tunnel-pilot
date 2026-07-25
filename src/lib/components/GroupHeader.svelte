<!--
  GroupHeader — collapsible section header for a tunnel group (spec 05 §4.2):
  chevron toggles collapse (persisted on the group model via update_group),
  a group-color swatch, X/Y active count, Start-all / Stop-all scoped to the
  group, and a hover ⋯ menu (Edit / Delete). A thin group-colored left-rail
  marks a group with ≥1 active tunnel (quiet ambient status).
-->
<script lang="ts">
  import Icon from "./ui/Icon.svelte";
  import Menu, { type MenuItem } from "./ui/Menu.svelte";
  import { groupColorVar } from "../ui/groupColors";

  interface Props {
    name: string;
    activeCount: number;
    total: number;
    collapsed: boolean;
    /** Persisted group color key; null → brand accent. Ungrouped passes null
        and omits onEdit/onDelete so it gets no swatch/menu. */
    color?: string | null;
    /** Ungrouped default section has no Start/Stop-all chrome when it's alone. */
    showBulk?: boolean;
    onToggle: () => void;
    onStartAll: () => void;
    onStopAll: () => void;
    /** Present only for real groups (not the Ungrouped bucket). */
    onEdit?: () => void;
    onDelete?: () => void;
  }

  const {
    name,
    activeCount,
    total,
    collapsed,
    color = null,
    showBulk = true,
    onToggle,
    onStartAll,
    onStopAll,
    onEdit,
    onDelete,
  }: Props = $props();

  const allActive = $derived(total > 0 && activeCount === total);
  const manageable = $derived(!!onEdit || !!onDelete);
  const rail = $derived(manageable ? groupColorVar(color) : "var(--accent)");

  let menuOpen = $state(false);
  const menuItems = $derived<MenuItem[]>([
    ...(onEdit
      ? [{ label: "Edit group…", icon: "pencil", run: onEdit } as MenuItem]
      : []),
    ...(onDelete
      ? [
          {
            label: "Delete group…",
            icon: "trash",
            danger: true,
            run: onDelete,
          } as MenuItem,
        ]
      : []),
  ]);
</script>

<div class="header" class:live={activeCount > 0} style="--rail: {rail}">
  <button
    type="button"
    class="disclosure"
    aria-expanded={!collapsed}
    onclick={onToggle}
  >
    <span class="chev" class:collapsed aria-hidden="true">
      <Icon name="chevron-down" size={14} />
    </span>
    {#if manageable}
      <span
        class="swatch"
        style="background: {groupColorVar(color)}"
        aria-hidden="true"
      ></span>
    {/if}
    <span class="name">{name}</span>
    <span class="count mono" class:active={activeCount > 0}>
      {activeCount}/{total}
    </span>
  </button>

  <div class="trailing">
    {#if showBulk && total > 0}
      <div class="bulk">
        {#if !allActive}
          <button type="button" class="bulk-btn" onclick={onStartAll}>
            <Icon name="play" size={13} /> Start all
          </button>
        {/if}
        {#if activeCount > 0}
          <button type="button" class="bulk-btn" onclick={onStopAll}>
            <Icon name="power" size={13} /> Stop all
          </button>
        {/if}
      </div>
    {/if}

    {#if menuItems.length > 0}
      <div class="menu-wrap">
        <button
          type="button"
          class="icon-btn"
          aria-label="{name} group actions"
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
    {/if}
  </div>
</div>

<style>
  .header {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-2) var(--sp-2) var(--sp-3);
    border-radius: var(--radius-sm);
  }
  /* Ambient group-colored left-rail when the group has a live tunnel. */
  .header.live::before {
    content: "";
    position: absolute;
    left: 0;
    top: var(--sp-2);
    bottom: var(--sp-2);
    width: var(--border-w-emph);
    border-radius: var(--radius-full);
    background: var(--rail);
  }
  .disclosure {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    flex: 1;
    min-width: 0;
    padding: var(--sp-1) 0;
    border: none;
    background: transparent;
    color: var(--text-2);
    cursor: pointer;
    text-align: left;
  }
  .disclosure:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
    border-radius: var(--radius-xs);
  }
  .chev {
    display: grid;
    place-items: center;
    color: var(--text-3);
    transition: transform var(--dur-fast) var(--ease-standard);
  }
  .chev.collapsed {
    transform: rotate(-90deg);
  }
  @media (prefers-reduced-motion: reduce) {
    .chev {
      transition: none;
    }
  }
  .swatch {
    flex: none;
    width: var(--sp-3);
    height: var(--sp-3);
    border-radius: var(--radius-full);
  }
  .name {
    font-size: var(--fs-label);
    line-height: var(--lh-label);
    font-weight: var(--fw-label);
    color: var(--text-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    font-size: var(--fs-mono-sm);
    color: var(--text-3);
  }
  .count.active {
    color: var(--status-connected-fg);
  }
  .trailing {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    flex: none;
  }
  .bulk {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    flex: none;
  }
  .bulk-btn {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    min-height: var(--hit-min);
    padding: var(--sp-1) var(--sp-2);
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-2);
    font-size: var(--fs-body-sm);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease-standard);
  }
  .bulk-btn:hover {
    background: var(--hover);
    color: var(--text);
  }
  .bulk-btn:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }
  .menu-wrap {
    position: relative;
    /* Kept mounted for layout stability; revealed on header hover / focus. */
    opacity: 0;
    transition: opacity var(--dur-fast) var(--ease-standard);
  }
  .header:hover .menu-wrap,
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
</style>
