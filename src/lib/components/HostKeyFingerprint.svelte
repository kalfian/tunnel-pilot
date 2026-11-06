<script lang="ts">
  import { copyText } from "../platform";
  import type { HostKeyPin } from "../types";
  import { pushToast } from "../ui/toast";
  import Button from "./ui/Button.svelte";

  /**
   * One host key as a term/description pair: the caption + algorithm, then
   * the SHA-256 fingerprint in mono. MUST sit inside a `<dl>` (the caller
   * owns the list so two keys read as one comparison). Fingerprints are long
   * base64 runs with no natural break, so they wrap anywhere instead of
   * overflowing a 390px viewport.
   */
  interface Props {
    /** "Pinned", "Server now presents", … */
    label: string;
    /** The key; null renders `missingText` instead (e.g. a known_hosts entry). */
    pin: HostKeyPin | null;
    missingText?: string;
    /** Accessible name of a copy button; omit for no copy button. */
    copyLabel?: string;
  }

  const { label, pin, missingText = "", copyLabel }: Props = $props();

  async function copy(): Promise<void> {
    if (!pin) return;
    try {
      await copyText(pin.fingerprint);
      pushToast("Fingerprint copied", { tone: "success" });
    } catch (err) {
      pushToast(`Copy failed: ${String(err)}`, { tone: "error" });
    }
  }
</script>

<div class="fp">
  <dt class="fp-term">
    <span class="fp-label">{label}</span>
    {#if pin}
      <span class="fp-alg mono">{pin.algorithm}</span>
    {/if}
  </dt>
  <dd class="fp-desc">
    {#if pin}
      <code class="fp-value mono selectable">{pin.fingerprint}</code>
      {#if copyLabel}
        <Button
          variant="ghost"
          size="sm"
          iconOnly="copy"
          ariaLabel={copyLabel}
          title="Copy fingerprint"
          onclick={() => void copy()}
        />
      {/if}
    {:else}
      <span class="fp-missing">{missingText}</span>
    {/if}
  </dd>
</div>

<style>
  .fp {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  .fp-term {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--sp-1) var(--sp-2);
  }
  .fp-label {
    font-size: var(--fs-label);
    line-height: var(--lh-label);
    font-weight: var(--fw-label);
    letter-spacing: var(--tracking-label);
    text-transform: uppercase;
    color: var(--text-2);
  }
  .fp-alg {
    font-size: var(--fs-mono-sm);
    line-height: var(--lh-mono-sm);
    color: var(--text-2);
  }
  .fp-desc {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    margin: 0;
    min-width: 0;
  }
  .fp-value {
    flex: 1;
    min-width: 0;
    padding: var(--sp-2) var(--sp-3);
    border-radius: var(--radius-xs);
    background: var(--surface-2);
    color: var(--text);
    font-size: var(--fs-mono);
    line-height: var(--lh-mono);
    /* base64 has no spaces: break anywhere rather than overflow. */
    overflow-wrap: anywhere;
    word-break: break-all;
  }
  .fp-missing {
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
    color: var(--text-2);
  }
</style>
