<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  export interface MenuItem {
    label: string;
    icon?: IconName;
    danger?: boolean;
    disabled?: boolean;
    run: () => void;
  }

  interface Props {
    items: MenuItem[];
    /** Fine-position; when omitted the menu anchors under its wrapper. */
    align?: "left" | "right";
    onClose: () => void;
  }

  const { items, align = "right", onClose }: Props = $props();

  let menuEl: HTMLDivElement | undefined = $state();
  let activeIndex = $state(0);

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
    focusActive();
  }

  function focusActive(): void {
    const el =
      menuEl?.querySelectorAll<HTMLElement>('[role="menuitem"]')[activeIndex];
    el?.focus();
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
      case "Home":
        e.preventDefault();
        activeIndex = enabled[0]?.i ?? 0;
        focusActive();
        break;
      case "End":
        e.preventDefault();
        activeIndex = enabled[enabled.length - 1]?.i ?? 0;
        focusActive();
        break;
      case "Escape":
        e.preventDefault();
        onClose();
        break;
    }
  }

  $effect(() => {
    activeIndex = enabled[0]?.i ?? 0;
    focusActive();
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
  role="menu"
  tabindex="-1"
  onkeydown={onKeydown}
>
  {#each items as item, i (item.label)}
    <button
      type="button"
      role="menuitem"
      class="mi"
      class:danger={item.danger}
      disabled={item.disabled}
      tabindex={i === activeIndex ? 0 : -1}
      onclick={() => {
        item.run();
        onClose();
      }}
      onmouseenter={() => (activeIndex = i)}
    >
      {#if item.icon}<Icon name={item.icon} size={14} />{/if}
      <span>{item.label}</span>
    </button>
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
