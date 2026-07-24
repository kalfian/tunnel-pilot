<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";

  interface Props {
    title: string;
    size?: "sm" | "md";
    /** Close via scrim/Esc/✕. The parent owns open/closed state. */
    onClose: () => void;
    /** Hide the header ✕ (e.g. destructive confirm keeps Cancel only). */
    showClose?: boolean;
    children: Snippet;
    footer?: Snippet;
  }

  const {
    title,
    size = "md",
    onClose,
    showClose = true,
    children,
    footer,
  }: Props = $props();

  const titleId = `dlg-${Math.random().toString(36).slice(2, 8)}`;

  let panel: HTMLDivElement | undefined = $state();
  let previouslyFocused: HTMLElement | null = null;

  function focusables(): HTMLElement[] {
    if (!panel) return [];
    return Array.from(
      panel.querySelectorAll<HTMLElement>(
        'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    );
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
      return;
    }
    if (e.key !== "Tab") return;
    const items = focusables();
    if (items.length === 0) return;
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement as HTMLElement | null;
    if (e.shiftKey && active === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  }

  $effect(() => {
    previouslyFocused = document.activeElement as HTMLElement | null;
    // Focus the first focusable inside the panel on open.
    const items = focusables();
    (items[0] ?? panel)?.focus();
    return () => previouslyFocused?.focus();
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div
  class="scrim"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) onClose();
  }}
>
  <div
    bind:this={panel}
    class="panel {size}"
    role="dialog"
    aria-modal="true"
    aria-labelledby={titleId}
    tabindex="-1"
  >
    <header class="head">
      <h2 id={titleId} class="title">{title}</h2>
      {#if showClose}
        <button
          type="button"
          class="close"
          aria-label="Close dialog"
          onclick={onClose}
        >
          <Icon name="x" size={16} />
        </button>
      {/if}
    </header>
    <div class="content">{@render children()}</div>
    {#if footer}
      <footer class="foot">{@render footer()}</footer>
    {/if}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: var(--z-scrim);
    background: var(--scrim);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 10vh var(--sp-6) var(--sp-6);
    overflow-y: auto;
    animation: scrim-in var(--dur-fast) var(--ease-standard);
  }
  .panel {
    z-index: var(--z-dialog);
    width: 100%;
    max-width: 460px;
    background: var(--surface-overlay);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
    animation: panel-in var(--dur-slow) var(--ease-decel);
  }
  .panel.sm {
    max-width: 380px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
    padding: var(--sp-5) var(--sp-5) var(--sp-4);
  }
  .title {
    margin: 0;
    font-size: var(--fs-title-md);
    line-height: var(--lh-title-md);
    font-weight: var(--fw-title-md);
    color: var(--text);
  }
  .close {
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
  .close:hover {
    background: var(--hover);
    color: var(--text);
  }
  .content {
    padding: 0 var(--sp-5);
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-3);
    padding: var(--sp-5);
  }

  @keyframes scrim-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
  @keyframes panel-in {
    from {
      opacity: 0;
      transform: translateY(-8px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .panel {
      animation: none;
    }
  }
</style>
