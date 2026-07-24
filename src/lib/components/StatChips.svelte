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
</script>

<div class="chips">
  <StatChip
    icon="arrow-left-right"
    value={conns}
    label="{conns} active connections"
  />
  <StatChip icon="arrow-up" value={up} label="{up} uploaded" />
  <StatChip icon="arrow-down" value={down} label="{down} downloaded" />
  <StatChip icon="gauge" value={latency} label="Latency {latency}" />
  <StatChip icon="clock" value={uptime} label="Uptime {uptime}" />
</div>

<style>
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }
</style>
