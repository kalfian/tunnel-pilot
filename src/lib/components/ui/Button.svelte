<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  interface Props {
    variant?: "primary" | "secondary" | "ghost" | "danger";
    size?: "sm" | "md";
    type?: "button" | "submit";
    iconLeft?: IconName;
    /** Icon-only button; `ariaLabel` becomes required for a11y. */
    iconOnly?: IconName;
    ariaLabel?: string;
    title?: string;
    loading?: boolean;
    disabled?: boolean;
    /** Fill the container width (dialog footers, empty-state CTAs). */
    block?: boolean;
    onclick?: (e: MouseEvent) => void;
    children?: Snippet;
  }

  const {
    variant = "secondary",
    size = "md",
    type = "button",
    iconLeft,
    iconOnly,
    ariaLabel,
    title,
    loading = false,
    disabled = false,
    block = false,
    onclick,
    children,
  }: Props = $props();

  const iconSize = $derived(size === "sm" ? 14 : 16);
</script>

<button
  {type}
  class="btn {variant} {size}"
  class:icon-only={!!iconOnly}
  class:block
  class:loading
  disabled={disabled || loading}
  aria-label={ariaLabel}
  aria-busy={loading}
  {title}
  {onclick}
>
  {#if loading}
    <span class="spinner" aria-hidden="true"></span>
  {:else if iconOnly}
    <Icon name={iconOnly} size={iconSize} />
  {:else if iconLeft}
    <Icon name={iconLeft} size={iconSize} />
  {/if}
  {#if !iconOnly}
    <span class="label" class:hidden={loading && !children}>
      {@render children?.()}
    </span>
  {/if}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    height: var(--btn-h);
    padding: 0 var(--sp-4);
    border-radius: var(--radius-sm);
    border: var(--border-w) solid transparent;
    font-size: var(--fs-body);
    font-weight: 500;
    line-height: 1;
    color: var(--text);
    background: transparent;
    cursor: pointer;
    white-space: nowrap;
    transition:
      background-color var(--dur-fast) var(--ease-standard),
      border-color var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .btn.sm {
    height: var(--btn-h-sm);
    padding: 0 var(--sp-3);
    font-size: var(--fs-body-sm);
  }
  .btn.block {
    width: 100%;
  }
  .btn.icon-only {
    padding: 0;
    width: var(--btn-h);
    min-width: var(--hit-min);
    color: var(--text-2);
  }
  .btn.icon-only.sm {
    width: var(--btn-h-sm);
  }

  /* primary — filled accent, white/dark text (AA via --accent-solid) */
  .primary {
    background: var(--accent-solid);
    color: var(--text-on-accent);
  }
  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .primary:active:not(:disabled) {
    background: var(--accent-active);
  }

  /* secondary — surface + hairline */
  .secondary {
    background: var(--surface);
    border-color: var(--border);
    color: var(--text);
  }
  .secondary:hover:not(:disabled) {
    background: var(--hover);
    border-color: var(--border-strong);
  }
  .secondary:active:not(:disabled) {
    background: var(--active);
  }

  /* ghost — transparent, hover tint */
  .ghost {
    background: transparent;
    color: var(--text-2);
  }
  .ghost:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .ghost:active:not(:disabled) {
    background: var(--active);
  }

  /* danger — destructive fill */
  .danger {
    background: var(--status-error);
    color: #fff;
  }
  .danger:hover:not(:disabled) {
    filter: brightness(0.94);
  }
  .danger:active:not(:disabled) {
    filter: brightness(0.88);
  }

  .btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .label.hidden {
    visibility: hidden;
  }

  .spinner {
    width: 14px;
    height: 14px;
    border-radius: var(--radius-full);
    border: 2px solid currentColor;
    border-top-color: transparent;
    opacity: 0.9;
    animation: btn-spin 0.7s linear infinite;
  }
  @keyframes btn-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      animation-duration: 1.4s;
    }
  }
</style>
