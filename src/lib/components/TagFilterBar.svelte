<!--
  TagFilterBar — toolbar tag filter (spec 05 §4.2). A dropdown of every tag in
  use with counts; picking one narrows the list (drives stores/groups.activeTag,
  single-tag per the store contract). The active tag shows as a removable pill.
-->
<script lang="ts">
  import Icon from "./ui/Icon.svelte";
  import TagPill from "./ui/TagPill.svelte";

  interface TagCount {
    name: string;
    count: number;
  }

  interface Props {
    tags: TagCount[];
    activeTag: string | null;
    onSelect: (tag: string | null) => void;
  }

  const { tags, activeTag, onSelect }: Props = $props();

  let open = $state(false);
  let wrapEl = $state<HTMLDivElement | undefined>();

  function pick(tag: string): void {
    onSelect(activeTag === tag ? null : tag);
    open = false;
  }

  $effect(() => {
    if (!open) return;
    function onDoc(e: MouseEvent): void {
      if (wrapEl && !wrapEl.contains(e.target as Node)) open = false;
    }
    document.addEventListener("mousedown", onDoc, true);
    return () => document.removeEventListener("mousedown", onDoc, true);
  });
</script>

<div class="bar">
  {#if activeTag}
    <TagPill label={activeTag} removable active onRemove={() => onSelect(null)} />
  {/if}
  <div class="wrap" bind:this={wrapEl}>
    <button
      type="button"
      class="trigger"
      class:on={activeTag !== null}
      aria-haspopup="listbox"
      aria-expanded={open}
      onclick={() => (open = !open)}
    >
      <Icon name="chevron-down" size={13} />
      <span>Tag</span>
    </button>
    {#if open}
      <ul class="menu" role="listbox" aria-label="Filter by tag">
        {#each tags as t (t.name)}
          <li>
            <button
              type="button"
              role="option"
              aria-selected={activeTag === t.name}
              class="opt"
              class:sel={activeTag === t.name}
              onclick={() => pick(t.name)}
            >
              <span class="opt-name">{t.name}</span>
              <span class="opt-count mono">{t.count}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .wrap {
    position: relative;
  }
  .trigger {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    height: var(--btn-h);
    padding: 0 var(--sp-3);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--text-2);
    font-size: var(--fs-body);
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease-standard),
      border-color var(--dur-fast) var(--ease-standard);
  }
  :global([data-theme="dark"]) .trigger {
    background: var(--surface-2);
  }
  .trigger:hover {
    border-color: var(--border-strong);
    color: var(--text);
  }
  .trigger.on {
    color: var(--accent-text);
    border-color: var(--border-strong);
  }
  .trigger:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
  }
  .menu {
    position: absolute;
    top: calc(100% + var(--sp-1));
    right: 0;
    min-width: 176px;
    max-height: 280px;
    overflow-y: auto;
    z-index: var(--z-dropdown);
    margin: 0;
    padding: var(--sp-2);
    list-style: none;
    background: var(--surface-overlay);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-2);
  }
  .opt {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    font-size: var(--fs-body);
    text-align: left;
    cursor: pointer;
  }
  .opt:hover {
    background: var(--hover);
  }
  .opt.sel {
    color: var(--accent-text);
  }
  .opt:focus-visible {
    outline: none;
    background: var(--hover);
  }
  .opt-count {
    flex: none;
    font-size: var(--fs-mono-sm);
    color: var(--text-3);
  }
</style>
