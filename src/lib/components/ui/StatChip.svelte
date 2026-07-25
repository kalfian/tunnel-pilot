<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  interface Props {
    icon: IconName;
    value: string;
    /** Accessible label ("3 active connections"). */
    label: string;
    /** Idle/zero value — render de-emphasized (tertiary) so live figures pop. */
    muted?: boolean;
  }

  const { icon, value, label, muted = false }: Props = $props();
</script>

<span class="chip" class:muted title={label} aria-label={label}>
  <Icon name={icon} size={12} />
  <!-- Value keyed so a changed snapshot cross-fades rather than jump-flashing. -->
  {#key value}
    <span class="value mono">{value}</span>
  {/key}
</span>

<style>
  /* Lean inline stat (spec 05 §4.3 anatomy): muted icon + mono value, no box.
     Five filled pills read as cramped/cluttered on the row — the connected
     stat line is a quiet meta row, not a group of buttons. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    color: var(--text-3);
    white-space: nowrap;
  }
  .value {
    font-size: var(--fs-mono-sm);
    line-height: var(--lh-mono-sm);
    color: var(--text-2);
    animation: chip-fade var(--dur-fast) var(--ease-standard);
  }
  /* Zero/idle: value drops to tertiary so the eye skips to active figures. */
  .chip.muted .value {
    color: var(--text-3);
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
