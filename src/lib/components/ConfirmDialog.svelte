<script lang="ts">
  import type { Snippet } from "svelte";
  import Dialog from "./ui/Dialog.svelte";
  import Button from "./ui/Button.svelte";

  /**
   * Confirm a consequential action (spec §9). The body is the caller's; this
   * owns the footer so every confirm behaves the same: Cancel is the default
   * focus (never auto-arm the risky action), Esc / scrim cancel, and the
   * confirm button shows a spinner while the action runs.
   */
  interface Props {
    title: string;
    /** Visible text of the confirm button. */
    confirmLabel: string;
    /** `danger` for destruction, `primary` for a deliberate, non-destructive commit. */
    confirmVariant?: "danger" | "primary";
    busy?: boolean;
    onConfirm: () => void;
    onClose: () => void;
    children: Snippet;
  }

  const {
    title,
    confirmLabel,
    confirmVariant = "danger",
    busy = false,
    onConfirm,
    onClose,
    children,
  }: Props = $props();

  let cancelBtn: HTMLButtonElement | undefined = $state();
  $effect(() => {
    // Default focus on the SAFE action (spec §9) — never auto-arm the risk.
    cancelBtn?.focus();
  });
</script>

<Dialog {title} size="sm" showClose={false} {onClose}>
  <div class="confirm-body">{@render children()}</div>

  {#snippet footer()}
    <button
      bind:this={cancelBtn}
      type="button"
      class="btn-cancel"
      onclick={onClose}
    >
      Cancel
    </button>
    <Button variant={confirmVariant} loading={busy} onclick={onConfirm}>
      {confirmLabel}
    </Button>
  {/snippet}
</Dialog>

<style>
  .confirm-body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding-bottom: var(--sp-1);
  }
  .confirm-body :global(p) {
    margin: 0;
    font-size: var(--fs-body);
    line-height: var(--lh-body);
    color: var(--text);
  }
  .confirm-body :global(p.muted) {
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }
  .confirm-body :global(p.warn) {
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--status-pending-fg);
  }
  /* The safe default action; styled as a secondary button. */
  .btn-cancel {
    height: var(--btn-h);
    padding: 0 var(--sp-4);
    border-radius: var(--radius-sm);
    border: var(--border-w) solid var(--border);
    background: var(--surface);
    color: var(--text);
    font-size: var(--fs-body);
    font-weight: 500;
    cursor: pointer;
    transition: background-color var(--dur-fast) var(--ease-standard);
  }
  .btn-cancel:hover {
    background: var(--hover);
    border-color: var(--border-strong);
  }
  .btn-cancel:active {
    background: var(--active);
  }
</style>
