/**
 * Pure formatters for technical/numeric values rendered in the mono stack
 * (spec 05 §4.3, design-tokens §4). Kept side-effect-free so they're unit
 * testable and reused across the card, stat chips, and log view.
 */

/** Human byte count, base-1000 (matches v1 + `ls -h` intuition). */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB", "PB"];
  const exp = Math.min(units.length - 1, Math.floor(Math.log10(bytes) / 3));
  const value = bytes / Math.pow(1000, exp);
  const digits = exp === 0 ? 0 : value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[exp]}`;
}

/** Compact uptime from an RFC3339 start timestamp. `2h 14m`, `41s`, `3d 2h`. */
export function formatUptime(connectedSince: string | null): string {
  if (!connectedSince) return "—";
  const startMs = Date.parse(connectedSince);
  if (Number.isNaN(startMs)) return "—";
  const secs = Math.max(0, Math.floor((Date.now() - startMs) / 1000));
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

/** Latency in ms, or an em dash when we have no sample yet. */
export function formatLatency(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms)) return "—";
  return `${Math.round(ms)} ms`;
}

/** `local:port → remoteHost:port` for the card route line. */
export function formatRoute(
  localBindAddress: string,
  localPort: number,
  remoteHost: string,
  remotePort: number,
): string {
  return `${localBindAddress}:${localPort} → ${remoteHost}:${remotePort}`;
}
