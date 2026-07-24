<script lang="ts" generics="T extends string | number">
  import Icon from "./Icon.svelte";

  interface Option {
    value: T;
    label: string;
  }

  interface Props {
    value: T;
    options: Option[];
    id?: string;
    ariaLabel?: string;
    disabled?: boolean;
    onchange?: (value: T) => void;
  }

  const { value, options, id, ariaLabel, disabled, onchange }: Props = $props();

  function handle(e: Event): void {
    const raw = (e.currentTarget as HTMLSelectElement).value;
    const match = options.find((o) => String(o.value) === raw);
    if (match) onchange?.(match.value);
  }
</script>

<!--
  Small enum picker built on a native <select> for full keyboard + AT support,
  restyled to the token system. A native control is the crafted choice here
  (type-ahead, screen-reader semantics for free) — see spec 05 §11 Select.
-->
<div class="select" class:disabled>
  <select
    {id}
    {disabled}
    aria-label={ariaLabel}
    value={String(value)}
    onchange={handle}
  >
    {#each options as opt (opt.value)}
      <option value={String(opt.value)}>{opt.label}</option>
    {/each}
  </select>
  <span class="chevron" aria-hidden="true">
    <Icon name="chevron-down" size={14} />
  </span>
</div>

<style>
  .select {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  select {
    appearance: none;
    height: var(--input-h);
    padding: 0 var(--sp-7) 0 var(--sp-3);
    border-radius: var(--radius-sm);
    border: var(--border-w) solid var(--border);
    background: var(--surface);
    color: var(--text);
    font-family: var(--font-ui);
    font-size: var(--fs-body);
    cursor: pointer;
    transition:
      border-color var(--dur-fast) var(--ease-standard),
      box-shadow var(--dur-fast) var(--ease-standard);
  }
  :global([data-theme="dark"]) select {
    background: var(--surface-2);
  }
  select:hover:not(:disabled) {
    border-color: var(--border-strong);
  }
  select:focus-visible {
    outline: none;
    border-width: var(--border-w-emph);
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus-ring-halo);
  }
  .chevron {
    position: absolute;
    right: var(--sp-3);
    display: grid;
    place-items: center;
    color: var(--text-2);
    pointer-events: none;
  }
  .select.disabled {
    opacity: 0.5;
  }
</style>
