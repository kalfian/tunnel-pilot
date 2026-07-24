<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  interface Props {
    icon: IconName;
    value: string;
    /** Accessible label ("3 active connections"). */
    label: string;
  }

  const { icon, value, label }: Props = $props();
</script>

<span class="chip" title={label} aria-label={label}>
  <Icon name={icon} size={12} />
  <!-- Value keyed so a changed snapshot cross-fades rather than jump-flashing. -->
  {#key value}
    <span class="value mono">{value}</span>
  {/key}
</span>

<style>
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-xs);
    background: var(--surface-2);
    color: var(--text-2);
  }
  .value {
    font-size: var(--fs-mono-sm);
    line-height: var(--lh-mono-sm);
    color: var(--text);
    animation: chip-fade var(--dur-fast) var(--ease-standard);
  }
  @keyframes chip-fade {
    from {
      opacity: 0.35;
    }
    to {
      opacity: 1;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .value {
      animation: none;
    }
  }
</style>
