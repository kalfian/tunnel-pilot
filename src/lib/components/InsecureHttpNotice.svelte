<!--
  Web mode over plain HTTP from a non-loopback host (spec 02 §10): a persistent,
  non-blocking warning. Not dismissible — the risk does not go away until the
  server sits behind HTTPS. Same amber "pending" tone as the keychain warning,
  so the app has one visual language for "works, but not safely".
-->
<script lang="ts">
  import Icon from "./ui/Icon.svelte";

  interface Props {
    /** `strip`: full-width bar above the content. `inline`: inside a card. */
    layout?: "strip" | "inline";
    /** Lets a field reference the notice via aria-describedby. */
    id?: string;
  }

  const { layout = "strip", id }: Props = $props();
</script>

<div class="notice {layout}" {id} role="note">
  <span class="glyph"><Icon name="lock-open" size={14} /></span>
  <p class="text">
    <strong>Not encrypted.</strong>
    <span class="long">
      This page uses plain HTTP, so traffic — including the admin password — can
      be read on the network. Serve Tunnel Pilot through an HTTPS reverse proxy.
    </span>
    <span class="short">
      Plain HTTP exposes traffic and the admin password. Use an HTTPS proxy.
    </span>
  </p>
</div>

<style>
  .notice {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    background: var(--status-pending-bg);
    color: var(--status-pending-fg);
    font-size: var(--fs-body-sm);
    line-height: var(--lh-body-sm);
  }
  .strip {
    flex: none;
    padding: var(--sp-2) var(--sp-6);
    border-bottom: var(--border-w) solid var(--divider);
  }
  .inline {
    padding: var(--sp-3);
    border-radius: var(--radius-sm);
  }
  /* One text line tall, so the glyph centres on the first line however the
     sentence wraps. */
  .glyph {
    display: inline-flex;
    align-items: center;
    flex: none;
    height: var(--lh-body-sm);
  }
  .text {
    margin: 0;
    min-width: 0;
  }
  .text strong {
    font-weight: 600;
  }
  /* Narrow app column (phones): the same warning in two lines, not four.
     Exactly one span is displayed, so assistive tech reads one version. */
  .short {
    display: none;
  }
  @container content (max-width: 440px) {
    .strip .long {
      display: none;
    }
    .strip .short {
      display: inline;
    }
  }
</style>
