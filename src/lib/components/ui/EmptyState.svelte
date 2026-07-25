<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import type { IconName } from "./icons";

  interface Props {
    icon: IconName;
    title: string;
    body: string;
    /** CTA + optional secondary link slot. */
    action?: Snippet;
  }

  const { icon, title, body, action }: Props = $props();
</script>

<div class="empty">
  <span class="glyph" aria-hidden="true">
    <Icon name={icon} size={40} stroke={1.5} />
  </span>
  <h2 class="title">{title}</h2>
  <p class="body">{body}</p>
  {#if action}
    <div class="actions">{@render action()}</div>
  {/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    text-align: center;
    gap: var(--sp-3);
    padding: var(--sp-9) var(--sp-6);
    height: 100%;
  }
  .glyph {
    color: var(--text-3);
    margin-bottom: var(--sp-2);
  }
  .title {
    margin: 0;
    font-size: var(--fs-title-md);
    line-height: var(--lh-title-md);
    font-weight: var(--fw-title-md);
    color: var(--text);
  }
  .body {
    margin: 0;
    max-width: 34ch;
    font-size: var(--fs-body);
    line-height: var(--lh-body);
    color: var(--text-2);
  }
  .actions {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--sp-3);
    margin-top: var(--sp-3);
  }
</style>
