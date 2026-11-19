<script lang="ts">
  import { tick } from "svelte";
  import type { Variable } from "../types";
  import { deleteVariable } from "../ipc";
  import { variables } from "../stores/variables";
  import { forwards } from "../stores/forwards";
  import { toUpdateErrorMessage } from "../stores/updater";
  import { pushToast } from "../ui/toast";
  import { variableToken, variableUses, type VariableUse } from "../validation";
  import Button from "./ui/Button.svelte";
  import Icon from "./ui/Icon.svelte";
  import Dialog from "./ui/Dialog.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import VariableFormDialog from "./VariableFormDialog.svelte";

  /**
   * Settings → Variables (spec 03 §24). Lists every variable with how many
   * tunnels use it; add/edit through `VariableFormDialog`, delete through the
   * shared confirm. Rendered on desktop and web alike — the same backend
   * contract serves both.
   */

  interface Row {
    v: Variable;
    uses: VariableUse[];
  }

  const rows = $derived<Row[]>(
    $variables.map((v) => ({ v, uses: variableUses($forwards, v.name) })),
  );

  type FormState = { mode: "add" } | { mode: "edit"; variable: Variable };
  let form = $state<FormState | null>(null);
  let deleteTarget = $state<Row | null>(null);
  let deleting = $state(false);
  let addWrap: HTMLSpanElement | undefined = $state();

  const tunnels = (n: number): string => `${n} tunnel${n === 1 ? "" : "s"}`;

  function useRole(u: VariableUse): string {
    if (u.inUsername && u.asPassword) return "username + password";
    return u.inUsername ? "in username" : "as password";
  }

  async function confirmDelete(): Promise<void> {
    const target = deleteTarget;
    if (!target || deleting) return;
    deleting = true;
    try {
      await deleteVariable(target.v.name);
      pushToast(`Deleted ${target.v.name}`, { tone: "success" });
    } catch (err) {
      // Refused while in use (a tunnel may have started using it since the
      // list rendered); the backend's message names the tunnels.
      pushToast(
        toUpdateErrorMessage(err) || `Couldn't delete ${target.v.name}`,
        { tone: "error" },
      );
    } finally {
      deleting = false;
      deleteTarget = null;
    }
    // The row's Delete button is about to disappear; land somewhere stable.
    await tick();
    addWrap?.querySelector("button")?.focus();
  }
</script>

<section class="group" aria-labelledby="vars-head">
  <div class="head">
    <h2 class="overline" id="vars-head">Variables</h2>
    <span bind:this={addWrap}>
      <Button
        size="sm"
        iconLeft="plus"
        onclick={() => (form = { mode: "add" })}
      >
        Add variable
      </Button>
    </span>
  </div>
  <p class="intro">
    Define a value once and use it in many tunnels: a plain variable inside an
    SSH username as <span class="mono tok">{variableToken("NAME")}</span>, a
    secret one as the password. Connected tunnels pick up a change when they
    reconnect.
  </p>

  {#if rows.length === 0}
    <div class="empty">
      <span class="empty-glyph" aria-hidden="true">
        <Icon name="braces" size={20} />
      </span>
      <div class="empty-text">
        <p class="empty-title">No variables yet</p>
        <p class="empty-body">
          Add <span class="mono tok">DB_USER</span> once and log in as
          <span class="mono tok">{variableToken("DB_USER")}</span> from every tunnel
          that shares it. Rotating it is then a single edit.
        </p>
      </div>
    </div>
  {:else}
    <ul class="rows" aria-label="Variables">
      {#each rows as row (row.v.name)}
        {@const v = row.v}
        {@const n = row.uses.length}
        <li class="var">
          <div class="v-main">
            <span class="v-name mono selectable">{variableToken(v.name)}</span>
            <span class="v-meta">
              {#if !v.secret}
                <span class="v-kind">Plain</span>
                <span class="sep" aria-hidden="true">·</span>
                <span class="v-value mono selectable" title={v.value}
                  >{v.value}</span
                >
              {:else if v.hasStoredValue}
                <span class="v-kind secret">
                  <Icon name="lock" size={12} />
                  Secret · stored
                </span>
              {:else}
                <span class="v-kind missing">
                  <Icon name="alert-triangle" size={12} />
                  Secret · no value yet
                </span>
              {/if}
              <span class="sep" aria-hidden="true">·</span>
              <span
                class="v-uses"
                title={n > 0
                  ? row.uses.map((u) => u.forwardName).join(", ")
                  : undefined}
              >
                {n === 0 ? "Not used" : `Used by ${tunnels(n)}`}
              </span>
            </span>
          </div>
          <div class="v-actions">
            <Button
              variant="ghost"
              size="sm"
              iconOnly="pencil"
              ariaLabel="Edit {v.name}"
              onclick={() => (form = { mode: "edit", variable: v })}
            />
            <Button
              variant="ghost"
              size="sm"
              iconOnly="trash"
              ariaLabel="Delete {v.name}"
              onclick={() => (deleteTarget = row)}
            />
          </div>
        </li>
      {/each}
    </ul>
  {/if}
</section>

{#if form}
  <VariableFormDialog
    variable={form.mode === "edit" ? form.variable : undefined}
    onClose={() => (form = null)}
  />
{/if}

{#if deleteTarget && deleteTarget.uses.length > 0}
  {@const t = deleteTarget}
  <Dialog
    title="{t.v.name} is in use"
    size="sm"
    onClose={() => (deleteTarget = null)}
  >
    <div class="in-use">
      <p>
        <span class="mono">{variableToken(t.v.name)}</span> can't be deleted
        while
        {t.uses.length === 1 ? "a tunnel uses" : "these tunnels use"} it:
      </p>
      <ul class="use-list">
        {#each t.uses as u (u.forwardId)}
          <li>
            <span class="use-name">{u.forwardName}</span>
            <span class="use-role">{useRole(u)}</span>
          </li>
        {/each}
      </ul>
      <p class="muted">
        Edit {t.uses.length === 1 ? "it" : "them"} to use something else first.
      </p>
    </div>
    {#snippet footer()}
      <Button variant="primary" onclick={() => (deleteTarget = null)}>
        OK
      </Button>
    {/snippet}
  </Dialog>
{:else if deleteTarget}
  {@const t = deleteTarget}
  <ConfirmDialog
    title="Delete variable?"
    confirmLabel="Delete"
    busy={deleting}
    onConfirm={() => void confirmDelete()}
    onClose={() => (deleteTarget = null)}
  >
    <p>
      <span class="mono">{variableToken(t.v.name)}</span> will be deleted. No tunnel
      uses it.
    </p>
    {#if t.v.secret}
      <p class="muted">Its stored secret is removed too.</p>
    {/if}
  </ConfirmDialog>
{/if}

<style>
  .group {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  .head .overline {
    margin: 0;
  }
  .intro {
    margin: 0;
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
    max-width: 60ch;
  }
  /* A `{{NAME}}` token inside prose: mono, one step brighter than the prose. */
  .tok {
    font-size: var(--fs-mono-sm);
    color: var(--text);
    white-space: nowrap;
  }

  .empty {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-4);
    padding: var(--sp-5);
    border: var(--border-w) dashed var(--border-strong);
    border-radius: var(--radius-md);
  }
  .empty-glyph {
    display: flex;
    flex: none;
    color: var(--text-2);
  }
  .empty-text {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  .empty-title {
    margin: 0;
    font-size: var(--fs-title-sm);
    line-height: var(--lh-title-sm);
    font-weight: var(--fw-title-sm);
    color: var(--text);
  }
  .empty-body {
    margin: 0;
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }

  .rows {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .var {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
    padding: var(--sp-4) 0;
    border-bottom: var(--border-w) solid var(--divider);
  }
  .var:last-child {
    border-bottom: none;
  }
  .v-main {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  .v-name {
    font-size: var(--fs-mono);
    line-height: var(--lh-mono);
    font-weight: 600;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  /* One line: only the plain value gives way (ellipsis, full value in its
     title) — a wrapped line would strand its "·" separators. */
  .v-meta {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    white-space: nowrap;
    min-width: 0;
    overflow: hidden;
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }
  .v-kind,
  .v-uses,
  .sep {
    flex: none;
  }
  .v-kind {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
  }
  .v-kind.missing {
    color: var(--status-pending-fg);
  }
  .v-value {
    flex: 0 1 auto;
    min-width: 0;
    font-size: var(--fs-mono-sm);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sep {
    color: var(--text-3);
  }
  .v-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    flex: none;
  }

  .in-use {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding-bottom: var(--sp-1);
  }
  .in-use p {
    margin: 0;
    font-size: var(--fs-body);
    line-height: var(--lh-body);
    color: var(--text);
  }
  .in-use p.muted {
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }
  .use-list {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-sm);
  }
  .use-list li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: var(--sp-2) var(--sp-3);
    border-bottom: var(--border-w) solid var(--divider);
  }
  .use-list li:last-child {
    border-bottom: none;
  }
  .use-name {
    min-width: 0;
    font-size: var(--fs-body);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .use-role {
    flex: none;
    font-size: var(--fs-body-sm);
    color: var(--text-2);
  }
</style>
