/**
 * Forwards store — read-through mirror of Rust `AppState.configs` plus the
 * per-tunnel status/stats maps (spec 02 §5). Source of truth is Rust; these are
 * kept in sync by events (see `lib/hydrate.ts`). Never authoritative on the
 * frontend.
 *
 * Reconciled from:
 *   - `forwards://changed`  → {@link applyForwards} (full list re-set)
 *   - `tunnel://status`     → {@link applyStatus}
 *   - `tunnel://stats`      → {@link applyStats}
 *   - `app_hydrate` snapshot runtimes → {@link applyRuntimes}
 */

import { derived, writable } from "svelte/store";
import type {
  ForwardConfig,
  ForwardRuntime,
  ForwardStatus,
  TunnelStats,
} from "../types";
import type { TunnelStatsEvent, TunnelStatusEvent } from "../events";

/** Ordered forward list (array order === display order, spec 04 §9). */
export const forwards = writable<ForwardConfig[]>([]);

/** Live status per forward id. Every id in {@link forwards} has an entry. */
export const statusById = writable<Record<string, ForwardStatus>>({});

/** Live stats per forward id. Every id in {@link forwards} has an entry. */
export const statsById = writable<Record<string, TunnelStats>>({});

/** Last error message per forward id (null when none). */
export const lastErrorById = writable<Record<string, string | null>>({});

/** Neutral stats used to seed a forward that has no live runtime yet. */
export const EMPTY_STATS: TunnelStats = {
  activeConnections: 0,
  totalBytesUp: 0,
  totalBytesDown: 0,
  lastPingLatencyMs: null,
  connectedSince: null,
};

/**
 * Replace the whole forward list (from `forwards://changed` or hydrate).
 *
 * Reconciles the per-id maps to the new set of ids: prunes entries for removed
 * forwards and seeds neutral defaults for new ones, while preserving live
 * status/stats for ids that survive (a reorder/rename must not clobber a live
 * tunnel's status).
 */
export function applyForwards(list: ForwardConfig[]): void {
  forwards.set(list);
  const ids = list.map((f) => f.id);
  statusById.update((prev) => reconcile(ids, prev, "disconnected"));
  statsById.update((prev) => reconcile(ids, prev, EMPTY_STATS));
  lastErrorById.update((prev) => reconcile(ids, prev, null));
}

/** Apply a `tunnel://status` transition to a single tunnel. */
export function applyStatus(e: TunnelStatusEvent): void {
  statusById.update((m) => ({ ...m, [e.id]: e.status }));
  lastErrorById.update((m) => ({ ...m, [e.id]: e.lastError }));
}

/** Apply a `tunnel://stats` update to a single tunnel. */
export function applyStats(e: TunnelStatsEvent): void {
  statsById.update((m) => ({ ...m, [e.id]: e.stats }));
}

/**
 * Seed the maps from an `app_hydrate` snapshot's `runtimes` list. Call AFTER
 * {@link applyForwards} so ids without a live runtime keep their seeded
 * defaults.
 */
export function applyRuntimes(runtimes: [string, ForwardRuntime][]): void {
  const status: Record<string, ForwardStatus> = {};
  const stats: Record<string, TunnelStats> = {};
  const errors: Record<string, string | null> = {};
  for (const [id, rt] of runtimes) {
    status[id] = rt.status;
    stats[id] = rt.stats;
    errors[id] = rt.lastError;
  }
  statusById.update((m) => ({ ...m, ...status }));
  statsById.update((m) => ({ ...m, ...stats }));
  lastErrorById.update((m) => ({ ...m, ...errors }));
}

/** Number of tunnels currently `connected` — feeds the tray/UI count. */
export const connectedCount = derived(
  statusById,
  ($statusById) =>
    Object.values($statusById).filter((s) => s === "connected").length,
);

/**
 * Rebuild a per-id map for exactly `ids`, keeping existing values and filling
 * `fallback` for ids not previously present.
 */
function reconcile<T>(
  ids: string[],
  prev: Record<string, T>,
  fallback: T,
): Record<string, T> {
  const next: Record<string, T> = {};
  for (const id of ids) {
    next[id] = id in prev ? prev[id] : fallback;
  }
  return next;
}
