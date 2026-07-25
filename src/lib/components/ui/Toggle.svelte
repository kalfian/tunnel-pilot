<script lang="ts">
  interface Props {
    checked: boolean;
    disabled?: boolean;
    /** Transitional (connecting/disconnecting): knob mid-travel + spinner ring. */
    pending?: boolean;
    ariaLabel: string;
    onchange?: (next: boolean) => void;
  }

  const {
    checked,
    disabled = false,
    pending = false,
    ariaLabel,
    onchange,
  }: Props = $props();

  function toggle(): void {
    if (disabled || pending) return;
    onchange?.(!checked);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === " " || e.key === "Enter") {
      e.preventDefault();
      toggle();
    }
  }
</script>

<button
  type="button"
  role="switch"
  aria-checked={checked}
  aria-label={ariaLabel}
  aria-busy={pending}
  disabled={disabled || pending}
  class="toggle"
  class:on={checked}
  class:pending
  onclick={toggle}
  {onkeydown}
>
  <span class="knob" class:mid={pending}>
    {#if pending}
      <span class="ring" aria-hidden="true"></span>
    {/if}
  </span>
</button>

<style>
  .toggle {
    position: relative;
    flex: none;
    width: var(--toggle-w);
    height: var(--toggle-h);
    padding: 0;
    border: none;
    border-radius: var(--radius-full);
    background: var(--surface-3);
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease-standard);
  }
  .toggle.on {
    background: var(--accent);
  }
  .toggle.pending {
    background: var(--status-pending);
  }
  /* Pending stays vivid (it's a live transitional signal); only a truly
     disabled toggle dims. */
  .toggle:disabled:not(.pending) {
    opacity: 0.5;
  }
  .toggle:disabled {
    cursor: not-allowed;
  }

  .knob {
    position: absolute;
    top: var(--toggle-pad);
    left: var(--toggle-pad);
    width: var(--toggle-knob);
    height: var(--toggle-knob);
    border-radius: var(--radius-full);
    background: var(--surface);
    box-shadow: var(--shadow-1);
    display: grid;
    place-items: center;
    transition: transform var(--dur-fast) var(--ease-spring);
  }
  .toggle.on .knob {
    transform: translateX(
      calc(var(--toggle-w) - var(--toggle-knob) - var(--toggle-pad) * 2)
    );
  }
  /* mid-travel for connecting/disconnecting */
  .knob.mid {
    transform: translateX(
      calc((var(--toggle-w) - var(--toggle-knob) - var(--toggle-pad) * 2) / 2)
    );
  }
  .toggle.on .knob.mid {
    transform: translateX(
      calc((var(--toggle-w) - var(--toggle-knob) - var(--toggle-pad) * 2) / 2)
    );
  }

  .ring {
    width: 10px;
    height: 10px;
    border-radius: var(--radius-full);
    border: 2px solid var(--status-pending);
    border-top-color: transparent;
    animation: knob-spin 0.7s linear infinite;
  }
  @keyframes knob-spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .knob {
      transition: none;
    }
    .ring {
      animation: none;
      border-top-color: var(--status-pending);
      opacity: 0.5;
    }
  }
</style>
