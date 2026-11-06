<script lang="ts">
  import { retryForward, trustHostKey } from "../ipc";
  import { clearHostKeyMismatch } from "../stores/forwards";
  import type { ForwardConfig, HostKeyMismatch } from "../types";
  import { formatHostPort } from "../ui/format";
  import { pushToast } from "../ui/toast";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import HostKeyFingerprint from "./HostKeyFingerprint.svelte";

  /**
   * Confirm before replacing a tunnel's host-key pin (spec 03 §23). Shows the
   * key exactly as the backend will pin it (`trust_host_key` pins the key from
   * the last refusal, never "whatever the next connection sees"), then
   * reconnects. Cancel is the default focus; nothing is sent until confirm.
   */
  interface Props {
    forward: ForwardConfig;
    mismatch: HostKeyMismatch;
    onClose: () => void;
    /** After a successful trust, before the reconnect resolves; the parent
        should unmount the dialog and move focus. */
    onTrusted?: (id: string) => void;
  }

  const { forward, mismatch, onClose, onTrusted }: Props = $props();

  let busy = $state(false);

  const hostPort = $derived(formatHostPort(forward.sshHost, forward.sshPort));

  async function confirm(): Promise<void> {
    if (busy) return;
    busy = true;
    // `forward` is a prop; capture the id so the reconnect below targets the
    // tunnel the user confirmed even if the parent unmounts us first.
    const id = forward.id;
    try {
      await trustHostKey(id);
    } catch (err) {
      pushToast(`Couldn't trust the new key: ${String(err)}`, {
        tone: "error",
      });
      busy = false;
      return;
    }
    // The backend cleared the pending refusal without a status event.
    clearHostKeyMismatch(id);
    onTrusted?.(id);
    try {
      await retryForward(id);
    } catch (err) {
      pushToast(`Key trusted, but reconnecting failed: ${String(err)}`, {
        tone: "error",
      });
    }
  }
</script>

<ConfirmDialog
  title="Trust the new host key?"
  confirmLabel="Trust and reconnect"
  confirmVariant="primary"
  {busy}
  onConfirm={() => void confirm()}
  {onClose}
>
  <p>
    <strong>{forward.name}</strong> connects to
    <span class="mono host">{hostPort}</span>, which now presents this key:
  </p>
  <dl class="keys">
    <HostKeyFingerprint
      label="Server now presents"
      pin={mismatch.presented}
      copyLabel="Copy the fingerprint {forward.name} now presents"
    />
  </dl>
  <p>
    Before you trust it, compare this fingerprint with the one the server's
    administrator gives you. If the key changed without a reason you know of,
    someone may be intercepting the connection.
  </p>
  <p class="muted">
    {#if mismatch.source === "knownHosts"}
      Trusting pins this key for this tunnel and reconnects. Your
      <span class="mono">~/.ssh/known_hosts</span> is not changed.
    {:else}
      Trusting replaces the pinned key for this tunnel and reconnects.
    {/if}
  </p>
</ConfirmDialog>

<style>
  .keys {
    margin: 0;
  }
  .host {
    overflow-wrap: anywhere;
  }
</style>
