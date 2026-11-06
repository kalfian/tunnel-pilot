<!--
  Web-mode shell (spec 02 §10): gates the app behind the admin password.
  `checking` paints only the canvas (no flash of the login form for a valid
  session); `signedIn` mounts the normal App and starts the live mirror;
  any 401 / socket 4401 tears the mirror down and returns here.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import App from "./App.svelte";
  import LoginScreen from "./lib/components/LoginScreen.svelte";
  import {
    authPhase,
    checkSession,
    serverUnreachable,
    sessionEnded,
    signIn,
    watchUnauthorized,
  } from "./lib/auth";
  import { startSync } from "./lib/hydrate";
  import { isInsecureRemote } from "./lib/platform";
  import { pushToast } from "./lib/ui/toast";

  const host = location.host;
  const insecure = isInsecureRemote();

  onMount(() => {
    const unwatch = watchUnauthorized();
    void checkSession();
    return unwatch;
  });

  // Live mirror only while signed in. startSync is async, so a sign-out that
  // lands before it resolves must still tear the subscriptions down.
  $effect(() => {
    if ($authPhase !== "signedIn") return;
    let cancelled = false;
    let stop: (() => void) | null = null;
    void startSync((err) => {
      if (!cancelled) {
        pushToast(`Couldn't load tunnels: ${String(err)}`, { tone: "error" });
      }
    }).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  });
</script>

{#if $authPhase === "signedIn"}
  <App />
{:else if $authPhase === "signedOut"}
  <LoginScreen
    {host}
    {insecure}
    sessionEnded={$sessionEnded}
    unreachable={$serverUnreachable}
    onSignIn={signIn}
  />
{:else}
  <div
    class="checking"
    role="status"
    aria-busy="true"
    aria-label="Loading"
  ></div>
{/if}

<style>
  .checking {
    height: 100%;
    background: var(--bg);
  }
</style>
