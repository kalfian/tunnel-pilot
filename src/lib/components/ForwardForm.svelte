<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { homeDir } from "@tauri-apps/api/path";
  import type { ForwardConfig, ForwardInput } from "../types";
  import {
    validateForwardForm,
    isFormValid,
    type ForwardFormField,
    type ForwardFormValues,
  } from "../validation";
  import {
    createForward,
    updateForward,
    setForwardPassword,
    clearForwardPassword,
  } from "../ipc";
  import { keychainUnavailable } from "../stores/settings";
  import { pushToast } from "../ui/toast";
  import Dialog from "./ui/Dialog.svelte";
  import Button from "./ui/Button.svelte";
  import Input from "./ui/Input.svelte";
  import SegmentedControl from "./ui/SegmentedControl.svelte";

  interface Props {
    mode: "add" | "edit";
    forward?: ForwardConfig;
    onClose: () => void;
    onSaved?: () => void;
  }

  const { mode, forward, onClose, onSaved }: Props = $props();

  type Tab = "general" | "advanced";
  type Auth = "password" | "identity";

  let tab = $state<Tab>("general");
  let saving = $state(false);
  let submitted = $state(false);
  let shake = $state(false);
  let confirmDiscard = $state(false);
  const touched = $state<Set<ForwardFormField>>(new Set());

  // --- Field state (strings for inputs; numbers parsed for validation) ---
  // The dialog is remounted per open, so `forward` never changes across its
  // life — capturing its initial values once is intentional.
  // svelte-ignore state_referenced_locally
  const c = forward;
  let name = $state(c?.name ?? "");
  let sshHost = $state(c?.sshHost ?? "");
  let sshPort = $state(String(c?.sshPort ?? 22));
  let sshUsername = $state(c?.sshUsername ?? "");
  let authMode = $state<Auth>(c?.identityFilePath ? "identity" : "password");
  let identityFilePath = $state(c?.identityFilePath ?? "");
  let password = $state("");
  let localBindAddress = $state(c?.localBindAddress ?? "127.0.0.1");
  let localPort = $state(c ? String(c.localPort) : "");
  let remoteHost = $state(c?.remoteHost ?? "");
  let remotePort = $state(c ? String(c.remotePort) : "");
  let keepAliveIntervalSec = $state(String(c?.keepAliveIntervalSec ?? 30));
  let keepAliveMaxCount = $state(String(c?.keepAliveMaxCount ?? 5));

  const hadStoredPassword = c?.hasStoredPassword ?? false;

  const num = (s: string): number => (s.trim() === "" ? NaN : Number(s));

  const values = $derived<ForwardFormValues>({
    name,
    sshHost,
    sshPort: num(sshPort),
    sshUsername,
    localBindAddress,
    localPort: num(localPort),
    remoteHost,
    remotePort: num(remotePort),
    keepAliveIntervalSec: num(keepAliveIntervalSec),
    keepAliveMaxCount: num(keepAliveMaxCount),
    identityFilePath: authMode === "identity" ? identityFilePath || null : null,
    password: authMode === "password" ? password : "",
    hasStoredPassword: authMode === "password" ? hadStoredPassword : false,
  });

  const errors = $derived(validateForwardForm(values));
  const valid = $derived(isFormValid(errors));

  const dirty = $derived(
    mode === "add" ||
      name !== (c?.name ?? "") ||
      sshHost !== (c?.sshHost ?? "") ||
      sshPort !== String(c?.sshPort ?? 22) ||
      sshUsername !== (c?.sshUsername ?? "") ||
      authMode !== (c?.identityFilePath ? "identity" : "password") ||
      identityFilePath !== (c?.identityFilePath ?? "") ||
      password !== "" ||
      localBindAddress !== (c?.localBindAddress ?? "127.0.0.1") ||
      localPort !== (c ? String(c.localPort) : "") ||
      remoteHost !== (c?.remoteHost ?? "") ||
      remotePort !== (c ? String(c.remotePort) : "") ||
      keepAliveIntervalSec !== String(c?.keepAliveIntervalSec ?? 30) ||
      keepAliveMaxCount !== String(c?.keepAliveMaxCount ?? 5),
  );

  // Show an error only after the field is touched or a submit was attempted.
  const showErr = (f: ForwardFormField): string | undefined =>
    submitted || touched.has(f) ? errors[f] : undefined;

  function touch(f: ForwardFormField): void {
    touched.add(f);
  }

  const AUTH_ON_ADVANCED: ForwardFormField[] = [
    "keepAliveIntervalSec",
    "keepAliveMaxCount",
  ];

  async function browse(): Promise<void> {
    try {
      const home = await homeDir();
      const picked = await open({
        multiple: false,
        directory: false,
        defaultPath: `${home}/.ssh`,
        title: "Select identity file",
      });
      if (typeof picked === "string") {
        identityFilePath = picked;
        touch("auth");
      }
    } catch (err) {
      pushToast(`Could not open file picker: ${String(err)}`, {
        tone: "error",
      });
    }
  }

  function toInput(): ForwardInput {
    return {
      name: name.trim(),
      sshHost: sshHost.trim(),
      sshPort: num(sshPort),
      sshUsername: sshUsername.trim(),
      identityFilePath:
        authMode === "identity" ? identityFilePath.trim() || null : null,
      localBindAddress: localBindAddress.trim(),
      localPort: num(localPort),
      remoteHost: remoteHost.trim(),
      remotePort: num(remotePort),
      keepAliveIntervalSec: num(keepAliveIntervalSec),
      keepAliveMaxCount: num(keepAliveMaxCount),
      groupId: c?.groupId ?? null,
      tags: c?.tags ?? [],
    };
  }

  async function applyPassword(id: string): Promise<void> {
    if (authMode === "password") {
      if (password.trim().length > 0) await setForwardPassword(id, password);
    } else if (hadStoredPassword) {
      // Switched away from a stored password → drop it.
      await clearForwardPassword(id);
    }
  }

  async function save(): Promise<void> {
    submitted = true;
    if (!valid) {
      // Errors on the Advanced tab shouldn't hide behind General.
      if (AUTH_ON_ADVANCED.some((f) => errors[f])) tab = "advanced";
      shake = true;
      setTimeout(() => (shake = false), 220);
      focusFirstError();
      return;
    }
    saving = true;
    let result: ForwardConfig;
    try {
      const input = toInput();
      result =
        mode === "add"
          ? await createForward(input)
          : await updateForward(c!.id, input);
    } catch (err) {
      pushToast(`Save failed: ${String(err)}`, { tone: "error" });
      saving = false;
      return;
    }
    // The config is now persisted. A failure applying the password is a
    // SEPARATE, softer failure — the tunnel exists, it just has no stored
    // secret (F47: don't report "Save failed" when the config saved fine).
    try {
      await applyPassword(result.id);
    } catch (err) {
      pushToast(
        `Tunnel ${mode === "add" ? "created" : "saved"}, but the password wasn't stored: ${String(err)}`,
        { tone: "error" },
      );
      onSaved?.();
      onClose();
      return;
    }
    pushToast(mode === "add" ? "Tunnel added" : "Changes saved", {
      tone: "success",
    });
    onSaved?.();
    onClose();
  }

  function focusFirstError(): void {
    queueMicrotask(() => {
      const first = document.querySelector<HTMLElement>(
        ".form [aria-invalid='true']",
      );
      first?.focus();
    });
  }

  function requestClose(): void {
    if (dirty && !saving) confirmDiscard = true;
    else onClose();
  }

  function onKeydown(e: KeyboardEvent): void {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      void save();
    }
  }
</script>

<!-- ⌘↵ saves from anywhere in the (modal) form (spec §7 keyboard). -->
<svelte:window onkeydown={onKeydown} />

<Dialog
  title={mode === "add" ? "Add tunnel" : "Edit tunnel"}
  onClose={requestClose}
>
  <form
    class="form"
    class:shake
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <SegmentedControl
      value={tab}
      ariaLabel="Form sections"
      options={[
        { value: "general", label: "General" },
        { value: "advanced", label: "Advanced" },
      ]}
      onchange={(v) => (tab = v as Tab)}
    />

    <!-- Both panels stay mounted; we hide the inactive one so state is kept. -->
    <div class="panel" hidden={tab !== "general"}>
      <div class="row">
        <label class="lbl" for="f-name">Name</label>
        <Input
          id="f-name"
          bind:value={name}
          placeholder="e.g. Production DB"
          invalid={!!showErr("name")}
          errorText={showErr("name")}
          onkeydown={() => touch("name")}
        />
      </div>

      <p class="overline group-head">SSH server</p>
      <div class="row">
        <label class="lbl" for="f-host">Host</label>
        <Input
          id="f-host"
          bind:value={sshHost}
          placeholder="bastion.example.com"
          mono
          invalid={!!showErr("sshHost")}
          errorText={showErr("sshHost")}
          onkeydown={() => touch("sshHost")}
        />
      </div>
      <div class="grid-2">
        <div class="row">
          <label class="lbl" for="f-port">Port</label>
          <Input
            id="f-port"
            type="number"
            inputmode="numeric"
            min={1}
            max={65535}
            bind:value={sshPort}
            placeholder="22"
            mono
            invalid={!!showErr("sshPort")}
            errorText={showErr("sshPort")}
            onkeydown={() => touch("sshPort")}
          />
        </div>
        <div class="row">
          <label class="lbl" for="f-user">Username</label>
          <Input
            id="f-user"
            bind:value={sshUsername}
            placeholder="deploy"
            mono
            invalid={!!showErr("sshUsername")}
            errorText={showErr("sshUsername")}
            onkeydown={() => touch("sshUsername")}
          />
        </div>
      </div>

      <p class="overline group-head">Authentication</p>
      <SegmentedControl
        value={authMode}
        ariaLabel="Authentication method"
        options={[
          { value: "password", label: "Password" },
          { value: "identity", label: "Identity file" },
        ]}
        onchange={(v) => {
          authMode = v as Auth;
          touch("auth");
        }}
      />
      {#if authMode === "identity"}
        <div class="row">
          <label class="lbl" for="f-identity">Identity file</label>
          <div class="browse">
            <Input
              id="f-identity"
              bind:value={identityFilePath}
              placeholder="~/.ssh/id_ed25519"
              mono
            />
            <Button onclick={() => void browse()}>Browse…</Button>
          </div>
        </div>
      {:else}
        <div class="row">
          <label class="lbl" for="f-password">Password</label>
          <Input
            id="f-password"
            type="password"
            bind:value={password}
            placeholder={hadStoredPassword
              ? "•••••••• (stored — leave blank to keep)"
              : "Enter password"}
          />
          {#if $keychainUnavailable}
            <p class="hint warn">
              OS keychain unavailable — passwords are stored in a local
              plaintext fallback file.
            </p>
          {/if}
        </div>
      {/if}
      {#if showErr("auth")}
        <p class="hint err">{showErr("auth")}</p>
      {/if}

      <p class="overline group-head">Port forwarding</p>
      <div class="forwarding">
        <div class="fwd-col">
          <span class="overline sub">Local</span>
          <div class="fwd-pair">
            <Input
              ariaLabel="Local bind address"
              bind:value={localBindAddress}
              placeholder="127.0.0.1"
              mono
              invalid={!!showErr("localBindAddress")}
              errorText={showErr("localBindAddress")}
              onkeydown={() => touch("localBindAddress")}
            />
            <Input
              ariaLabel="Local port"
              type="number"
              inputmode="numeric"
              min={1}
              max={65535}
              bind:value={localPort}
              placeholder="5432"
              mono
              invalid={!!showErr("localPort")}
              errorText={showErr("localPort")}
              onkeydown={() => touch("localPort")}
            />
          </div>
        </div>
        <span class="arrow mono" aria-hidden="true">→</span>
        <div class="fwd-col">
          <span class="overline sub">Remote</span>
          <div class="fwd-pair">
            <Input
              ariaLabel="Remote host"
              bind:value={remoteHost}
              placeholder="10.0.4.12"
              mono
              invalid={!!showErr("remoteHost")}
              errorText={showErr("remoteHost")}
              onkeydown={() => touch("remoteHost")}
            />
            <Input
              ariaLabel="Remote port"
              type="number"
              inputmode="numeric"
              min={1}
              max={65535}
              bind:value={remotePort}
              placeholder="5432"
              mono
              invalid={!!showErr("remotePort")}
              errorText={showErr("remotePort")}
              onkeydown={() => touch("remotePort")}
            />
          </div>
        </div>
      </div>
    </div>

    <div class="panel" hidden={tab !== "advanced"}>
      <p class="overline group-head">Keep-alive</p>
      <div class="row">
        <label class="lbl" for="f-ka-interval">Interval (seconds)</label>
        <Input
          id="f-ka-interval"
          type="number"
          inputmode="numeric"
          min={0}
          bind:value={keepAliveIntervalSec}
          placeholder="30"
          mono
          invalid={!!showErr("keepAliveIntervalSec")}
          errorText={showErr("keepAliveIntervalSec")}
          onkeydown={() => touch("keepAliveIntervalSec")}
        />
        <p class="hint">How often to send a keep-alive. 0 uses the default.</p>
      </div>
      <div class="row">
        <label class="lbl" for="f-ka-max">Max unanswered</label>
        <Input
          id="f-ka-max"
          type="number"
          inputmode="numeric"
          min={0}
          bind:value={keepAliveMaxCount}
          placeholder="5"
          mono
          invalid={!!showErr("keepAliveMaxCount")}
          errorText={showErr("keepAliveMaxCount")}
          onkeydown={() => touch("keepAliveMaxCount")}
        />
        <p class="hint">Drop the tunnel after this many missed keep-alives.</p>
      </div>
    </div>

    <!-- Submit lives in the footer; this hidden button lets Enter submit. -->
    <button type="submit" class="sr-submit" tabindex="-1" aria-hidden="true"
    ></button>
  </form>

  {#snippet footer()}
    {#if confirmDiscard}
      <span class="discard-note">Discard unsaved changes?</span>
      <Button onclick={() => (confirmDiscard = false)}>Keep editing</Button>
      <Button variant="danger" onclick={onClose}>Discard</Button>
    {:else}
      <Button onclick={requestClose}>Cancel</Button>
      <Button
        variant="primary"
        loading={saving}
        disabled={!valid || (mode === "edit" && !dirty)}
        onclick={() => void save()}
      >
        {mode === "add" ? "Save tunnel" : "Save changes"}
      </Button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
    padding-bottom: var(--sp-2);
  }
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
  }
  .panel[hidden] {
    display: none;
  }
  .row {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-width: 0;
  }
  .lbl {
    font-size: var(--fs-body);
    font-weight: 500;
    color: var(--text);
  }
  .grid-2 {
    display: grid;
    grid-template-columns: 1fr 1.6fr;
    gap: var(--sp-4);
  }
  .group-head {
    margin: var(--sp-3) 0 0;
  }
  .browse {
    display: flex;
    gap: var(--sp-3);
    align-items: flex-start;
  }
  .browse :global(.field) {
    flex: 1;
  }
  .forwarding {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: end;
    gap: var(--sp-3);
  }
  .fwd-col {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-width: 0;
  }
  .sub {
    color: var(--text-3);
  }
  .fwd-pair {
    display: grid;
    grid-template-columns: 1.4fr 1fr;
    gap: var(--sp-2);
  }
  .arrow {
    padding-bottom: var(--sp-3);
    color: var(--text-2);
    font-size: var(--fs-title-md);
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
  .discard-note {
    margin-right: auto;
    font-size: var(--fs-body-sm);
    color: var(--text-2);
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
  .shake {
    animation: shake var(--dur-fast) var(--ease-standard) 0s 2;
  }
  @keyframes shake {
    0%,
    100% {
      transform: translateX(0);
    }
    25% {
      transform: translateX(-4px);
    }
    75% {
      transform: translateX(4px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .shake {
      animation: none;
    }
  }
</style>
