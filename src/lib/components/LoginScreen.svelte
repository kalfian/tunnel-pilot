<!--
  Web-mode sign-in (spec 02 §10). One field: the admin password set on the
  server with `tunnel-pilot-cli web set-password`. Presentational — the shell
  passes `onSignIn` and reacts to the auth store; this screen only reports.

  Announcements: focus stays in the field on Enter, so a failed attempt is
  spoken once through the assertive live region (always mounted), and the same
  text is the field's visible error + description for when focus returns.
-->
<script lang="ts">
  import type { SignInResult } from "../auth";
  import appIcon from "../../../assets/icons/app_icon_64.png";
  import Input from "./ui/Input.svelte";
  import Button from "./ui/Button.svelte";
  import Icon from "./ui/Icon.svelte";
  import InsecureHttpNotice from "./InsecureHttpNotice.svelte";

  interface Props {
    /** `location.host` — which server this password unlocks. */
    host: string;
    /** A live session just ended (expiry, server restart, sign-out elsewhere). */
    sessionEnded?: boolean;
    /** The session check could not reach the server. */
    unreachable?: boolean;
    /** Plain HTTP from a non-loopback host. */
    insecure?: boolean;
    onSignIn: (password: string) => Promise<SignInResult>;
  }

  const {
    host,
    sessionEnded = false,
    unreachable = false,
    insecure = false,
    onSignIn,
  }: Props = $props();

  const UNREACHABLE =
    "Can't reach the server. Check that tunnel-pilot-cli is running, then try again.";

  let password = $state("");
  let busy = $state(false);
  // Seeded once from the boot check; later attempts own it.
  // svelte-ignore state_referenced_locally
  let error = $state<string | null>(unreachable ? UNREACHABLE : null);
  let announcement = $state("");

  const describedby = $derived(
    [sessionEnded ? "login-ended" : "", insecure ? "login-insecure" : ""]
      .filter(Boolean)
      .join(" ") || undefined,
  );

  function waitText(seconds: number | null): string {
    if (seconds === null) return "Wait a few minutes, then try again.";
    if (seconds < 60) {
      return `Try again in ${seconds} second${seconds === 1 ? "" : "s"}.`;
    }
    const minutes = Math.ceil(seconds / 60);
    return `Try again in ${minutes} minute${minutes === 1 ? "" : "s"}.`;
  }

  function messageFor(result: Exclude<SignInResult, { ok: true }>): string {
    switch (result.reason) {
      case "invalid":
        return "Incorrect password.";
      case "rateLimited":
        return `Too many failed attempts. ${waitText(result.retryAfterSec)}`;
      case "unreachable":
        return UNREACHABLE;
      case "server":
        return `Sign-in failed: ${result.message}`;
    }
  }

  function fail(message: string): void {
    error = message;
    // Re-set even when unchanged so a repeated failure is spoken again.
    announcement = "";
    queueMicrotask(() => (announcement = message));
    const field = document.getElementById("login-password");
    if (field instanceof HTMLInputElement) {
      field.focus();
      field.select();
    }
  }

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    if (busy) return;
    if (password === "") {
      fail("Enter the admin password.");
      return;
    }
    busy = true;
    try {
      const result = await onSignIn(password);
      if (result.ok) {
        password = "";
        error = null;
        return;
      }
      fail(messageFor(result));
    } finally {
      busy = false;
    }
  }
</script>

<div class="page">
  <main class="card" aria-labelledby="login-title">
    <header class="brand">
      <img class="mark" src={appIcon} alt="" width="40" height="40" />
      <div class="brand-text">
        <h1 id="login-title" class="name">Tunnel Pilot</h1>
        <p class="where">
          Sign in to manage tunnels on <span class="host mono">{host}</span>
        </p>
      </div>
    </header>

    {#if insecure}
      <InsecureHttpNotice layout="inline" id="login-insecure" />
    {/if}

    {#if sessionEnded}
      <p class="ended" id="login-ended">
        <Icon name="info" size={14} />
        <span>Your session ended. Sign in again to continue.</span>
      </p>
    {/if}

    <form class="form" novalidate onsubmit={(e) => void submit(e)}>
      <label class="lbl" for="login-password">Admin password</label>
      <Input
        id="login-password"
        name="password"
        type="password"
        autocomplete="current-password"
        bind:value={password}
        invalid={error !== null}
        errorText={error ?? undefined}
        {describedby}
        autofocus
      />
      <Button type="submit" variant="primary" block loading={busy}>
        Sign in
      </Button>
    </form>

    <p class="help">
      Set or reset the password on the server:
      <code class="cmd mono selectable">tunnel-pilot-cli web set-password</code>
    </p>
  </main>

  <div class="sr-only" aria-live="assertive" aria-atomic="true">
    {announcement}
  </div>
</div>

<style>
  .page {
    height: 100%;
    overflow-y: auto;
    display: grid;
    justify-items: center;
    align-content: start;
    /* Sit a little above centre: a lone form at dead centre reads as floating. */
    padding: max(var(--sp-8), 14vh) var(--sp-5) var(--sp-8);
    background: var(--bg);
  }
  .card {
    width: min(100%, 360px);
    display: flex;
    flex-direction: column;
    gap: var(--sp-6);
    padding: var(--sp-7);
    background: var(--surface);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-1);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: var(--sp-4);
  }
  .mark {
    flex: none;
    border-radius: var(--radius-sm);
  }
  .brand-text {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  .name {
    margin: 0;
    font-size: var(--fs-title-lg);
    line-height: var(--lh-title-lg);
    font-weight: var(--fw-title-lg);
    letter-spacing: var(--tracking-tight);
  }
  .where {
    margin: 0;
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }
  .host {
    font-size: var(--fs-mono-sm);
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .ended {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin: 0;
    padding: var(--sp-3);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-2);
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .form :global(.btn) {
    margin-top: var(--sp-3);
  }
  .lbl {
    font-size: var(--fs-body);
    line-height: var(--lh-body);
    font-weight: 500;
    color: var(--text);
  }
  .help {
    margin: 0;
    padding-top: var(--sp-5);
    border-top: var(--border-w) solid var(--divider);
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--sp-2);
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }
  .cmd {
    max-width: 100%;
    padding: var(--sp-1) var(--sp-3);
    border-radius: var(--radius-xs);
    border: var(--border-w) solid var(--border);
    background: var(--surface-2);
    font-size: var(--fs-mono-sm);
    line-height: var(--lh-mono-sm);
    color: var(--text-2);
    overflow-wrap: anywhere;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>
