<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";
  import Self from "./Menu.svelte";

  export interface MenuItem {
    label: string;
    icon?: IconName;
    danger?: boolean;
    disabled?: boolean;
    /** Leaf action. Omitted when the item only opens a `submenu`. */
    run?: () => void;
    /** Nested flyout (e.g. "Assign group ▸"). Rendered to the side. */
    submenu?: MenuItem[];
  }

  interface Props {
    items: MenuItem[];
    /** Fine-position; when omitted the menu anchors under its wrapper. */
    align?: "left" | "right";
    /** Close the WHOLE popup (root closer — also passed to nested menus so a
        leaf selection collapses every level). */
    onClose: () => void;
    /** Close just THIS level (only set on a nested submenu — drives Esc / ←). */
    onBack?: () => void;
  }

  const { items, align = "right", onClose, onBack }: Props = $props();

  let menuEl: HTMLDivElement | undefined = $state();
  let activeIndex = $state(0);
  // Index of the item whose submenu is currently open (null = none).
  let openSub = $state<number | null>(null);

  const enabled = $derived(
    items.map((it, i) => ({ it, i })).filter((x) => !x.it.disabled),
  );

  function move(delta: number): void {
    if (enabled.length === 0) return;
    const positions = enabled.map((e) => e.i);
    let cur = positions.indexOf(activeIndex);
    if (cur === -1) cur = 0;
    const next = (cur + delta + positions.length) % positions.length;
    activeIndex = positions[next];
    openSub = null;
    focusActive();
  }

  function focusActive(): void {
    const el =
      menuEl?.querySelectorAll<HTMLElement>('[role="menuitem"]')[activeIndex];
    el?.focus();
  }

  function activate(item: MenuItem, i: number): void {
    if (item.disabled) return;
    if (item.submenu) {
      openSub = openSub === i ? null : i;
      return;
    }
    item.run?.();
    onClose();
  }

  function onKeydown(e: KeyboardEvent): void {
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        move(-1);
        break;
      case "ArrowRight": {
        const item = items[activeIndex];
        if (item?.submenu) {
          e.preventDefault();
          openSub = activeIndex;
        }
        break;
      }
      case "ArrowLeft":
        if (onBack) {
          e.preventDefault();
          onBack();
        }
        break;
      case "Home":
        e.preventDefault();
        activeIndex = enabled[0]?.i ?? 0;
        openSub = null;
        focusActive();
        break;
      case "End":
        e.preventDefault();
        activeIndex = enabled[enabled.length - 1]?.i ?? 0;
        openSub = null;
        focusActive();
        break;
      case "Escape":
        e.preventDefault();
        if (openSub !== null) openSub = null;
        else if (onBack) onBack();
        else onClose();
        break;
    }
  }

  $effect(() => {
    activeIndex = enabled[0]?.i ?? 0;
    focusActive();
    // Only the ROOT menu (no onBack) watches for outside clicks; nested menus
    // live inside the root's DOM, so the root's containment check covers them.
    if (onBack) return;
    function onDocClick(e: MouseEvent): void {
      if (menuEl && !menuEl.contains(e.target as Node)) onClose();
    }
    document.addEventListener("mousedown", onDocClick, true);
    return () => document.removeEventListener("mousedown", onDocClick, true);
  });
</script>

<div
  bind:this={menuEl}
  class="menu {align}"
  class:nested={!!onBack}
  role="menu"
  tabindex="-1"
  onkeydown={onKeydown}
>
  {#each items as item, i (item.label)}
    <div class="mi-wrap">
      <button
        type="button"
        role="menuitem"
        class="mi"
        class:danger={item.danger}
        class:has-sub={!!item.submenu}
        aria-haspopup={item.submenu ? "menu" : undefined}
        aria-expanded={item.submenu ? openSub === i : undefined}
        disabled={item.disabled}
        tabindex={i === activeIndex ? 0 : -1}
        onclick={() => activate(item, i)}
        onmouseenter={() => {
          activeIndex = i;
          if (item.submenu) openSub = i;
          else openSub = null;
        }}
      >
        {#if item.icon}<Icon name={item.icon} size={14} />{/if}
        <span class="mi-label">{item.label}</span>
        {#if item.submenu}
          <span class="mi-chev" aria-hidden="true">
            <Icon name="chevron-right" size={14} />
          </span>
        {/if}
      </button>

      {#if item.submenu && openSub === i}
        <div class="sub">
          <Self
            items={item.submenu}
            align="left"
            {onClose}
            onBack={() => {
              openSub = null;
              focusActive();
            }}
          />
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  .menu {
    position: absolute;
    top: calc(100% + var(--sp-1));
    min-width: 176px;
    z-index: var(--z-dropdown);
    padding: var(--sp-2);
    background: var(--surface-overlay);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-2);
    display: flex;
    flex-direction: column;
    gap: 1px;
    animation: menu-in var(--dur-fast) var(--ease-decel);
  }
  .menu.right {
    right: 0;
  }
  .menu.left {
    left: 0;
  }
  /* A nested flyout anchors to the SIDE of its parent item, not under it. */
  .menu.nested {
    top: calc(-1 * var(--sp-2));
    left: calc(100% + var(--sp-1));
    right: auto;
  }
  .mi-wrap {
    position: relative;
    display: flex;
  }
  .sub {
    position: absolute;
    top: 0;
    left: 0;
  }
  .mi {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    font-size: var(--fs-body);
    text-align: left;
    cursor: pointer;
  }
  .mi-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mi-chev {
    display: grid;
    place-items: center;
    color: var(--text-3);
    margin-right: calc(-1 * var(--sp-1));
  }
  .mi:hover:not(:disabled),
  .mi:focus-visible {
    background: var(--hover);
    outline: none;
  }
  .mi.danger {
    color: var(--status-error-fg);
  }
  .mi.danger:hover:not(:disabled),
  .mi.danger:focus-visible {
    background: var(--status-error-bg);
  }
  .mi:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  @keyframes menu-in {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .menu {
      animation: none;
    }
  }
</style>
