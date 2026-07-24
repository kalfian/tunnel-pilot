<script lang="ts">
  interface Props {
    value: string;
    id?: string;
    type?: "text" | "number" | "password";
    placeholder?: string;
    invalid?: boolean;
    /** Shown below the field, in error color, and wired to aria-describedby. */
    errorText?: string;
    disabled?: boolean;
    mono?: boolean;
    inputmode?: "text" | "numeric";
    min?: number;
    max?: number;
    step?: number;
    ariaLabel?: string;
    autofocus?: boolean;
    onkeydown?: (e: KeyboardEvent) => void;
  }

  let {
    value = $bindable(),
    id,
    type = "text",
    placeholder,
    invalid = false,
    errorText,
    disabled = false,
    mono = false,
    inputmode,
    min,
    max,
    step,
    ariaLabel,
    autofocus = false,
    onkeydown,
  }: Props = $props();

  const errorId = $derived(id ? `${id}-error` : undefined);

  let el: HTMLInputElement | undefined = $state();
  $effect(() => {
    // Programmatic focus (avoids the static `autofocus` attribute lint flags).
    if (autofocus && el) el.focus();
  });
</script>

<div class="field">
  <input
    bind:this={el}
    {id}
    {type}
    {placeholder}
    {disabled}
    {min}
    {max}
    {step}
    {inputmode}
    class="input"
    class:mono
    class:invalid={invalid && !!errorText}
    aria-label={ariaLabel}
    aria-invalid={invalid && !!errorText}
    aria-describedby={invalid && errorText ? errorId : undefined}
    {value}
    oninput={(e) => (value = e.currentTarget.value)}
    {onkeydown}
  />
  {#if invalid && errorText}
    <span class="error" id={errorId}>{errorText}</span>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-width: 0;
  }
  .input {
    width: 100%;
    height: var(--input-h);
    padding: 0 var(--sp-3);
    border-radius: var(--radius-sm);
    border: var(--border-w) solid var(--border);
    background: var(--surface);
    color: var(--text);
    font-family: var(--font-ui);
    font-size: var(--fs-body);
    line-height: var(--lh-body);
    transition:
      border-color var(--dur-fast) var(--ease-standard),
      box-shadow var(--dur-fast) var(--ease-standard);
  }
  .input.mono {
    font-family: var(--font-mono);
    font-variant-numeric: tabular-nums;
  }
  .input::placeholder {
    color: var(--text-3);
  }
  .input:hover:not(:disabled):not(:focus) {
    border-color: var(--border-strong);
  }
  .input:focus {
    outline: none;
    border-width: var(--border-w-emph);
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--focus-ring-halo);
  }
  .input.invalid {
    border-color: var(--status-error);
  }
  .input.invalid:focus {
    box-shadow: 0 0 0 3px var(--status-error-bg);
  }
  .input:disabled {
    background: var(--surface-2);
    color: var(--text-3);
    cursor: not-allowed;
  }
  /* Dark: inputs read better with a filled inset (design-tokens §10). */
  :global([data-theme="dark"]) .input {
    background: var(--surface-2);
    border-color: var(--border);
  }
  .error {
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--status-error-fg);
  }
  /* Hide the native number spinner (custom, quiet look). */
  .input[type="number"]::-webkit-inner-spin-button,
  .input[type="number"]::-webkit-outer-spin-button {
    appearance: none;
    margin: 0;
  }
</style>
