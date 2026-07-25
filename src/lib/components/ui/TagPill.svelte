<script lang="ts">
  import Icon from "./Icon.svelte";

  interface Props {
    label: string;
    /** Renders a ✕ affordance and calls onRemove when clicked. */
    removable?: boolean;
    /** Active-filter styling (used in the toolbar filter pill). */
    active?: boolean;
    onRemove?: () => void;
  }

  const { label, removable = false, active = false, onRemove }: Props = $props();
</script>

{#if removable}
  <button
    type="button"
    class="pill removable"
    class:active
    onclick={onRemove}
    aria-label="Remove filter {label}"
  >
    <span class="label">{label}</span>
    <Icon name="x" size={11} />
  </button>
{:else}
  <span class="pill" class:active>{label}</span>
{/if}

<style>
  .pill {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    max-width: 120px;
    padding: 2px var(--sp-2);
    border-radius: var(--radius-full);
    background: var(--surface-2);
    color: var(--text-2);
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pill.active {
    background: var(--accent-subtle-2);
    color: var(--accent-text);
  }
  button.pill {
    border: none;
    cursor: pointer;
  }
  button.pill:hover {
    color: var(--text);
  }
  button.pill.active:hover {
    color: var(--accent-text);
    filter: brightness(0.95);
  }
  button.pill:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }
</style>
