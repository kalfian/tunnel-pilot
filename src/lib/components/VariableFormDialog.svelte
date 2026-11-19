<script lang="ts">
  import { SvelteSet } from "svelte/reactivity";
  import { isWeb } from "../runtime";
  import type { Variable, VariableInput } from "../types";
  import { saveVariable } from "../ipc";
  import { variables } from "../stores/variables";
  import { forwards } from "../stores/forwards";
  import { keychainUnavailable } from "../stores/settings";
  import { toUpdateErrorMessage } from "../stores/updater";
  import { pushToast } from "../ui/toast";
  import {
    validateVariableForm,
    variableToken,
    type VariableFormField,
  } from "../validation";
  import Dialog from "./ui/Dialog.svelte";
  import Button from "./ui/Button.svelte";
  import Input from "./ui/Input.svelte";
  import SegmentedControl from "./ui/SegmentedControl.svelte";

  /**
   * Add or edit a variable (spec 03 §24). The name is the identity, so it is
   * read-only when editing. A secret value lives only in this dialog's
   * password field until submit — it is never read back, never put in a
   * store, and the field is emptied on submit and on close.
   */
  interface Props {
    /** The variable to edit; omit to add one. */
    variable?: Variable;
    onClose: () => void;
  }

  const { variable, onClose }: Props = $props();

  type Kind = "plain" | "secret";

  // The dialog is remounted per open, so `variable` never changes across its
  // life — capturing the initial values once is intentional.
  // svelte-ignore state_referenced_locally
  const existing = variable;
  const editing = existing !== undefined;

  let name = $state(existing?.name ?? "");
  let kind = $state<Kind>(existing?.secret ? "secret" : "plain");
  // Separate fields per kind: switching kind never moves a plain value into
  // the secret channel or echoes a typed secret as plain text.
  let plainValue = $state(existing && !existing.secret ? existing.value : "");
  let secretValue = $state("");
  let saving = $state(false);
  let submitted = $state(false);
  let serverError = $state<string | null>(null);
  const touched = new SvelteSet<VariableFormField>();

  const keepsStoredSecret = $derived(
    kind === "secret" &&
      existing?.secret === true &&
      existing.hasStoredValue === true,
  );

  const errors = $derived(
    validateVariableForm(
      {
        name,
        secret: kind === "secret",
        value: kind === "secret" ? secretValue : plainValue,
      },
      { existing, variables: $variables, forwards: $forwards },
    ),
  );
  const valid = $derived(Object.keys(errors).length === 0);

  const dirty = $derived(
    !existing ||
      (kind === "secret") !== existing.secret ||
      (kind === "secret" ? secretValue !== "" : plainValue !== existing.value),
  );

  // A field with content shows its error live (a paste has no keydown); an
  // empty one waits for a keystroke or a submit, so "required" never greets.
  function hasContent(f: VariableFormField): boolean {
    if (f === "name") return name !== "";
    if (f === "value")
      return (kind === "secret" ? secretValue : plainValue) !== "";
    return false;
  }
  const showErr = (f: VariableFormField): string | undefined =>
    submitted || touched.has(f) || hasContent(f) ? errors[f] : undefined;

  const trimmedName = $derived(name.trim());
  const shownToken = $derived(variableToken(trimmedName || "NAME"));

  function close(): void {
    secretValue = "";
    onClose();
  }

  async function save(): Promise<void> {
    submitted = true;
    serverError = null;
    if (!valid || saving || (editing && !dirty)) return;
    const secret = kind === "secret";
    const input: VariableInput = {
      name: existing?.name ?? trimmedName,
      secret,
      // Secret: a blank field keeps the stored value (null).
      value: secret ? (secretValue === "" ? null : secretValue) : plainValue,
    };
    saving = true;
    try {
      await saveVariable(input);
    } catch (err) {
      // The backend's message names the problem (in use, name taken, …).
      serverError =
        toUpdateErrorMessage(err) || "The variable couldn't be saved.";
      saving = false;
      return;
    }
    secretValue = "";
    saving = false;
    pushToast(editing ? "Variable saved" : "Variable added", {
      tone: "success",
    });
    onClose();
  }
</script>

<Dialog title={editing ? "Edit variable" : "Add variable"} onClose={close}>
  <form
    class="form"
    novalidate
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <div class="row">
      {#if existing}
        <span class="lbl">Name</span>
        <p class="static mono">
          {variableToken(existing.name)}
        </p>
        <p class="hint">
          Names can't change. To rename, add a new variable and point the
          tunnels at it.
        </p>
      {:else}
        <label class="lbl" for="v-name">Name</label>
        <Input
          id="v-name"
          bind:value={name}
          placeholder="DB_USER"
          mono
          autocomplete="off"
          describedby="v-name-hint"
          invalid={!!showErr("name")}
          errorText={showErr("name")}
          onkeydown={() => touched.add("name")}
        />
        <p class="hint" id="v-name-hint">
          Letters, digits and _. Written as <span class="mono token"
            >{shownToken}</span
          >.
        </p>
      {/if}
    </div>

    <div class="row">
      <span class="lbl" aria-hidden="true">Kind</span>
      <SegmentedControl
        value={kind}
        ariaLabel="Kind"
        options={[
          { value: "plain", label: "Plain" },
          { value: "secret", label: "Secret", icon: "lock" },
        ]}
        onchange={(v) => {
          kind = v as Kind;
          serverError = null;
        }}
      />
      {#if errors.secret}
        <p class="hint err" role="alert">{errors.secret}</p>
      {:else if kind === "plain"}
        <p class="hint">
          Shown in the app. Use it in an SSH username as <span
            class="mono token">{shownToken}</span
          >.
        </p>
      {:else}
        <p class="hint">
          {isWeb ? "Kept in the server's keychain" : "Kept in the OS keychain"} and
          never shown again. A tunnel can use it as its password.
        </p>
      {/if}
    </div>

    {#if kind === "plain"}
      <div class="row">
        <label class="lbl" for="v-value">Value</label>
        <Input
          id="v-value"
          bind:value={plainValue}
          placeholder="e.g. deploy"
          mono
          autocomplete="off"
          invalid={!!showErr("value")}
          errorText={showErr("value")}
          onkeydown={() => touched.add("value")}
        />
      </div>
    {:else}
      <div class="row">
        <label class="lbl" for="v-secret">Secret value</label>
        <Input
          id="v-secret"
          type="password"
          bind:value={secretValue}
          placeholder={keepsStoredSecret
            ? "•••••••• stored — leave blank to keep"
            : "Enter the secret value"}
          autocomplete="off"
          describedby={keepsStoredSecret ? "v-secret-hint" : undefined}
          invalid={!!showErr("value")}
          errorText={showErr("value")}
          onkeydown={() => touched.add("value")}
        />
        {#if keepsStoredSecret}
          <p class="hint" id="v-secret-hint">
            Leave blank to keep the current value; typing replaces it.
          </p>
        {/if}
        {#if $keychainUnavailable}
          <p class="hint warn">
            {#if isWeb}
              No keychain on the server — secret values are stored there in a
              plaintext fallback file.
            {:else}
              OS keychain unavailable — secret values are stored in a local
              plaintext fallback file.
            {/if}
          </p>
        {/if}
      </div>
    {/if}

    {#if serverError}
      <p class="server-err" role="alert">{serverError}</p>
    {/if}

    <!-- Submit lives in the footer; this hidden button lets Enter submit. -->
    <button type="submit" class="sr-submit" tabindex="-1" aria-hidden="true"
    ></button>
  </form>

  {#snippet footer()}
    <Button onclick={close}>Cancel</Button>
    <Button
      variant="primary"
      loading={saving}
      disabled={!valid || (editing && !dirty)}
      onclick={() => void save()}
    >
      {editing ? "Save changes" : "Add variable"}
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
    align-items: stretch;
    gap: var(--sp-2);
    min-width: 0;
  }
  .row :global(.segmented) {
    align-self: flex-start;
  }
  .lbl {
    font-size: var(--fs-body);
    font-weight: 500;
    color: var(--text);
  }
  /* Read-only name: the token as it is written in a username. */
  .static {
    margin: 0;
    font-size: var(--fs-mono);
    line-height: var(--lh-mono);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .hint {
    margin: 0;
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }
  .hint.err {
    color: var(--status-error-fg);
  }
  .hint.warn {
    color: var(--status-pending-fg);
  }
  .token {
    font-size: var(--fs-mono-sm);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .server-err {
    margin: 0;
    padding: var(--sp-3);
    border-radius: var(--radius-sm);
    background: var(--status-error-bg);
    color: var(--status-error-fg);
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
  }
  .sr-submit {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    border: 0;
    padding: 0;
  }
</style>
