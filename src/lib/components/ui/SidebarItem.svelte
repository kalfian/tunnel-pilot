<script lang="ts">
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  interface Props {
    icon: IconName;
    label: string;
    active: boolean;
    /** Numeric badge (e.g. active connection count on Connections). */
    badge?: number;
    /** Icon-only compact rail (< 640 content width). */
    compact?: boolean;
    onclick?: () => void;
  }

  const {
    icon,
    label,
    active,
    badge,
    compact = false,
    onclick,
  }: Props = $props();
</script>

<button
  type="button"
  class="item"
  class:active
  class:compact
  aria-current={active ? "page" : undefined}
  aria-label={compact ? label : undefined}
  title={compact ? label : undefined}
  {onclick}
>
  <span class="rail" aria-hidden="true"></span>
  <Icon name={icon} size={17} />
  {#if !compact}<span class="label">{label}</span>{/if}
  {#if badge !== undefined && badge > 0}
    <span class="badge mono" aria-label="{badge} active">{badge}</span>
  {/if}
</button>

<style>
  .item {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    height: 34px;
    padding: 0 var(--sp-3);
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-2);
    font-size: var(--fs-body);
    font-weight: 500;
    text-align: left;
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease-standard),
      color var(--dur-fast) var(--ease-standard);
  }
  .item.compact {
    justify-content: center;
    padding: 0;
    width: 34px;
  }
  .item:hover:not(.active) {
    background: var(--hover);
    color: var(--text);
  }
  .item.active {
    background: var(--accent-subtle);
    color: var(--accent-text);
  }
  .rail {
    position: absolute;
    left: 0;
    top: 50%;
    transform: translateY(-50%) scaleY(0);
    width: var(--border-w-emph);
    height: 18px;
    border-radius: var(--radius-full);
    background: var(--accent);
    transition: transform var(--dur-fast) var(--ease-standard);
  }
  .item.active .rail {
    transform: translateY(-50%) scaleY(1);
  }
  .label {
    flex: 1;
    min-width: 0;
  }
  .badge {
    flex: none;
    min-width: 18px;
    height: 18px;
    padding: 0 var(--sp-2);
    display: grid;
    place-items: center;
    border-radius: var(--radius-full);
    background: var(--accent-subtle-2);
    color: var(--accent-text);
    font-size: var(--fs-mono-sm);
  }
  .item.compact .badge {
    position: absolute;
    top: 1px;
    right: 1px;
    min-width: 15px;
    height: 15px;
    padding: 0;
    font-size: 9px;
  }
</style>
