<!--
  GroupFormDialog — create or rename/recolor a tunnel group (spec 05 §4.2).
  Name input + a small palette of tokened colors. Dispatches createGroup /
  updateGroup via lib/ipc. ⌘↵ saves, Esc closes (Dialog owns the trap + Esc).
-->
<script lang="ts">
  import type { TunnelGroup } from "../types";
  import { createGroup, updateGroup } from "../ipc";
  import { GROUP_COLORS, groupColorVar } from "../ui/groupColors";
  import { pushToast } from "../ui/toast";
  import Dialog from "./ui/Dialog.svelte";
  import Button from "./ui/Button.svelte";
  import Input from "./ui/Input.svelte";

  interface Props {
    mode: "add" | "edit";
    group?: TunnelGroup;
    onClose: () => void;
  }

  const { mode, group, onClose }: Props = $props();

  // Dialog is remounted per open, so capturing the group once is intentional.
  // svelte-ignore state_referenced_locally
  let name = $state(group?.name ?? "");
  // svelte-ignore state_referenced_locally
  let color = $state<string | null>(group?.color ?? null);
  let saving = $state(false);

  const trimmed = $derived(name.trim());
  const valid = $derived(trimmed !== "");
  const dirty = $derived(
    mode === "add" ||
      trimmed !== (group?.name ?? "") ||
      color !== (group?.color ?? null),
  );

  async function save(): Promise<void> {
    if (!valid || saving) return;
    saving = true;
    try {
      if (mode === "add") {
        await createGroup({ name: trimmed, color, collapsed: false });
        pushToast("Group created", { tone: "success" });
      } else if (group) {
        await updateGroup(group.id, {
          name: trimmed,
          color,
          collapsed: group.collapsed,
        });
        pushToast("Group updated", { tone: "success" });
      }
      onClose();
    } catch (err) {
      pushToast(`Save failed: ${String(err)}`, { tone: "error" });
      saving = false;
    }
  }

  // Roving arrow-key selection across the color radiogroup.
  const swatches = $derived<{ key: string | null; label: string }[]>([
    { key: null, label: "None" },
    ...GROUP_COLORS.map((c) => ({ key: c.key, label: c.label })),
  ]);

  function onSwatchKeydown(e: KeyboardEvent, index: number): void {
    let next = index;
    if (e.key === "ArrowRight" || e.key === "ArrowDown") next = index + 1;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp") next = index - 1;
    else return;
    e.preventDefault();
    next = (next + swatches.length) % swatches.length;
    color = swatches[next].key;
    const el = document.querySelectorAll<HTMLElement>(".swatch-btn")[next];
    el?.focus();
  }

  function onKeydown(e: KeyboardEvent): void {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      void save();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<Dialog
  title={mode === "add" ? "New group" : "Edit group"}
  size="sm"
  {onClose}
>
  <form
    class="form"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <div class="row">
      <label class="lbl" for="g-name">Name</label>
      <Input
        id="g-name"
        bind:value={name}
        placeholder="e.g. Production"
        autofocus
      />
    </div>

    <div class="row">
      <span class="lbl" id="g-color-lbl">Color</span>
      <div
        class="swatches"
        role="radiogroup"
        aria-labelledby="g-color-lbl"
      >
        {#each swatches as sw, i (sw.label)}
          <button
            type="button"
            class="swatch-btn"
            class:selected={color === sw.key}
            class:none={sw.key === null}
            role="radio"
            aria-checked={color === sw.key}
            aria-label={sw.label}
            title={sw.label}
            tabindex={color === sw.key ||
            (color === null && sw.key === null)
              ? 0
              : -1}
            style={sw.key === null
              ? undefined
              : `--sw: ${groupColorVar(sw.key)}`}
            onclick={() => (color = sw.key)}
            onkeydown={(e) => onSwatchKeydown(e, i)}
          >
            {#if sw.key === null}
              <span class="slash" aria-hidden="true"></span>
            {/if}
          </button>
        {/each}
      </div>
    </div>
  </form>

  {#snippet footer()}
    <Button onclick={onClose}>Cancel</Button>
    <Button
      variant="primary"
      loading={saving}
      disabled={!valid || (mode === "edit" && !dirty)}
      onclick={() => void save()}
    >
      {mode === "add" ? "Create group" : "Save changes"}
    </Button>
  {/snippet}
</Dialog>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-5);
    padding-bottom: var(--sp-2);
  }
  .row {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .lbl {
    font-size: var(--fs-body);
    font-weight: 500;
    color: var(--text);
  }
  .swatches {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }
  .swatch-btn {
    position: relative;
    width: var(--hit-min);
    height: var(--hit-min);
    padding: 0;
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-full);
    background: var(--sw, var(--surface-2));
    cursor: pointer;
    transition:
      transform var(--dur-fast) var(--ease-standard),
      box-shadow var(--dur-fast) var(--ease-standard);
  }
  .swatch-btn.none {
    background: var(--surface-2);
  }
  .swatch-btn:hover {
    transform: scale(1.08);
  }
  .swatch-btn.selected {
    box-shadow:
      0 0 0 2px var(--surface-overlay),
      0 0 0 4px var(--focus-ring);
  }
  .swatch-btn:focus-visible {
    outline: none;
    box-shadow:
      0 0 0 2px var(--surface-overlay),
      0 0 0 4px var(--focus-ring-halo);
  }
  /* Diagonal "no color" slash on the None swatch. */
  .slash {
    position: absolute;
    inset: 0;
    border-radius: var(--radius-full);
    background: linear-gradient(
      to top right,
      transparent calc(50% - 1px),
      var(--text-3) calc(50% - 1px),
      var(--text-3) calc(50% + 1px),
      transparent calc(50% + 1px)
    );
  }
  @media (prefers-reduced-motion: reduce) {
    .swatch-btn {
      transition: none;
    }
    .swatch-btn:hover {
      transform: none;
    }
  }
</style>
