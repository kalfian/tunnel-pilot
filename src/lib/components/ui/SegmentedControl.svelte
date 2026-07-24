<script lang="ts" generics="T extends string">
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  interface Segment {
    value: T;
    label: string;
    icon?: IconName;
  }

  interface Props {
    value: T;
    options: Segment[];
    ariaLabel: string;
    /** Icon-only segments still expose their label to AT. */
    compact?: boolean;
    onchange?: (value: T) => void;
  }

  const {
    value,
    options,
    ariaLabel,
    compact = false,
    onchange,
  }: Props = $props();
</script>

<div class="segmented" role="radiogroup" aria-label={ariaLabel}>
  {#each options as opt (opt.value)}
    <button
      type="button"
      role="radio"
      aria-checked={value === opt.value}
      aria-label={compact ? opt.label : undefined}
      class="seg"
      class:active={value === opt.value}
      title={compact ? opt.label : undefined}
      onclick={() => onchange?.(opt.value)}
    >
      {#if opt.icon}
        <Icon name={opt.icon} size={15} />
      {/if}
      {#if !compact}<span>{opt.label}</span>{/if}
    </button>
  {/each}
</div>

<style>
  .segmented {
    display: inline-flex;
    padding: var(--sp-1);
    gap: var(--sp-1);
    background: var(--surface-2);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-sm);
  }
  .seg {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    min-height: var(--hit-min);
    padding: 0 var(--sp-3);
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-2);
    font-size: var(--fs-body-sm);
    font-weight: 500;
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .seg:hover:not(.active) {
    color: var(--text);
    background: var(--hover);
  }
  .seg.active {
    background: var(--accent-subtle-2);
    color: var(--accent-text);
  }
</style>
