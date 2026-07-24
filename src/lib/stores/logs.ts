/**
 * Logs store — mirror of the Rust ring buffer (cap 500, newest-first), fed via
 * `log://line` / `log://cleared` events and the `app_hydrate` snapshot (see
 * `lib/hydrate.ts`, spec 02 §5, 03 §18).
 */

import { writable } from "svelte/store";
import type { LogEntry } from "../types";

/** Max entries retained on the frontend, mirroring the Rust buffer cap. */
export const LOG_CAP = 500;

/** Buffered log lines, newest-first. */
export const logs = writable<LogEntry[]>([]);

/** Append a new line (from `log://line`), keeping newest-first and capping. */
export function appendLogLine(entry: LogEntry): void {
  logs.update((prev) => {
    const next = [entry, ...prev];
    return next.length > LOG_CAP ? next.slice(0, LOG_CAP) : next;
  });
}

/**
 * Replace the whole buffer from the hydrate snapshot. The Rust buffer is
 * already newest-first and capped; we cap defensively.
 */
export function setLogs(entries: LogEntry[]): void {
  logs.set(entries.length > LOG_CAP ? entries.slice(0, LOG_CAP) : entries);
}

/** Empty the buffer (from `log://cleared`). */
export function resetLogs(): void {
  logs.set([]);
}
