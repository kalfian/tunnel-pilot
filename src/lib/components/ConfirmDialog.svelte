<script lang="ts">
  import Dialog from "./ui/Dialog.svelte";
  import Button from "./ui/Button.svelte";

  interface Props {
    /** The tunnel being removed. */
    name: string;
    /** Warn that a live tunnel will be dropped. */
    connected: boolean;
    busy?: boolean;
    onConfirm: () => void;
    onClose: () => void;
  }

  const { name, connected, busy = false, onConfirm, onClose }: Props = $props();

  let cancelBtn: HTMLButtonElement | undefined = $state();
  $effect(() => {
    // Default focus on the SAFE action (spec §9) — never auto-arm destruction.
    cancelBtn?.focus();
  });
</script>

<Dialog title="Delete tunnel?" size="sm" showClose={false} {onClose}>
  <p class="body">
    <strong>{name}</strong> will be permanently removed. This can't be undone.
  </p>
  {#if connected}
    <p class="warn">
      This tunnel is currently connected and will be disconnected.
    </p>
  {/if}

  {#snippet footer()}
    <button
      bind:this={cancelBtn}
      type="button"
      class="btn-cancel"
      onclick={onClose}
    >
      Cancel
    </button>
    <Button variant="danger" loading={busy} onclick={onConfirm}>Delete</Button>
  {/snippet}
</Dialog>

<style>
  .body {
    margin: 0 0 var(--sp-3);
    font-size: var(--fs-body);
    line-height: var(--lh-body);
    color: var(--text);
  }
  .warn {
    margin: 0 0 var(--sp-2);
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
</style>
