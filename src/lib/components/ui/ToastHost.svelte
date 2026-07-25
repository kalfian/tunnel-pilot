<script lang="ts">
  import { toasts, dismissToast } from "../../ui/toast";
  import Icon from "./Icon.svelte";

  const TONE_ICON = {
    info: "check",
    success: "check",
    error: "alert-triangle",
  } as const;
</script>

<div class="host" role="region" aria-label="Notifications">
  <div class="stack" aria-live="polite">
    {#each $toasts as t (t.id)}
      <div class="toast {t.tone}" role="status">
        <span class="ic" aria-hidden="true">
          <Icon name={TONE_ICON[t.tone]} size={15} />
        </span>
        <span class="msg">{t.message}</span>
        {#if t.action}
          <button
            type="button"
            class="action"
            onclick={() => {
              t.action?.run();
              dismissToast(t.id);
            }}
          >
            {t.action.label}
          </button>
        {/if}
        <button
          type="button"
          class="close"
          aria-label="Dismiss notification"
          onclick={() => dismissToast(t.id)}
        >
          <Icon name="x" size={13} />
        </button>
      </div>
    {/each}
  </div>
</div>

<style>
  .host {
    position: fixed;
    top: calc(var(--titlebar-h) + var(--sp-3));
    right: var(--sp-5);
    z-index: var(--z-toast);
    pointer-events: none;
  }
  .stack {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    width: 320px;
    max-width: calc(100vw - var(--sp-8));
  }
  .toast {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
    background: var(--surface-overlay);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-2);
    color: var(--text);
    font-size: var(--fs-body);
    pointer-events: auto;
    animation: toast-in var(--dur-slow) var(--ease-decel);
  }
  .toast.success .ic {
    color: var(--status-connected-fg);
  }
  .toast.error {
    border-color: var(--status-error);
  }
  .toast.error .ic {
    color: var(--status-error-fg);
  }
  .toast.info .ic {
    color: var(--accent-text);
  }
  .msg {
    flex: 1;
    min-width: 0;
  }
  .action {
    flex: none;
    border: none;
    background: transparent;
    color: var(--accent-text);
    font-size: var(--fs-body);
    font-weight: 500;
    cursor: pointer;
    padding: 0 var(--sp-1);
  }
  .action:hover {
    text-decoration: underline;
  }
  .close {
    flex: none;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-3);
    cursor: pointer;
  }
  .close:hover {
    background: var(--hover);
    color: var(--text);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateX(12px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .toast {
      animation: none;
    }
  }
</style>
