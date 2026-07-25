<script lang="ts">
  import type { TunnelStats } from "../types";
  import StatChip from "./ui/StatChip.svelte";
  import { formatBytes, formatLatency, formatUptime } from "../ui/format";

  interface Props {
    stats: TunnelStats;
  }
  const { stats }: Props = $props();

  const conns = $derived(String(stats.activeConnections));
  const up = $derived(formatBytes(stats.totalBytesUp));
  const down = $derived(formatBytes(stats.totalBytesDown));
  const latency = $derived(formatLatency(stats.lastPingLatencyMs));
  // Recomputes each 3s stats snapshot (new object) — advances the uptime clock.
  const uptime = $derived(formatUptime(stats.connectedSince));

  // De-emphasize figures that mean "nothing happening yet" so live numbers pop.
  const noConns = $derived(stats.activeConnections === 0);
  const noUp = $derived(stats.totalBytesUp <= 0);
  const noDown = $derived(stats.totalBytesDown <= 0);
  const noLatency = $derived(latency === "—");
</script>

<div class="chips">
  <StatChip
    icon="arrow-left-right"
    value={conns}
    label="{conns} active connections"
    muted={noConns}
  />
  <span class="sep" aria-hidden="true"></span>
  <StatChip icon="arrow-up" value={up} label="{up} uploaded" muted={noUp} />
  <StatChip
    icon="arrow-down"
    value={down}
    label="{down} downloaded"
    muted={noDown}
  />
  <span class="sep" aria-hidden="true"></span>
  <StatChip
    icon="gauge"
    value={latency}
    label="Latency {latency}"
    muted={noLatency}
  />
  <StatChip icon="clock" value={uptime} label="Uptime {uptime}" />
</div>

<style>
  /* Quiet inline meta row under the route. Even gaps, hairline separators
     grouping conns | traffic | timing; wraps to 2 rows on Compact. */
  .chips {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    column-gap: var(--sp-4);
    row-gap: var(--sp-2);
  }
  .sep {
    align-self: center;
    width: var(--border-w);
    height: var(--sp-4);
    background: var(--divider);
  }
  /* A separator that wraps to the start of a new row is noise — hide it. */
  .sep:first-child,
  .sep:last-child {
    display: none;
  }
</style>
