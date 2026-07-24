/**
 * Updater store — update availability + download progress (spec 02 §5).
 * Reconciled from `update://status` / `update://progress` and the `app_hydrate`
 * snapshot (see `lib/hydrate.ts`). The updater backend is live as of M6; this
 * store holds the reconciled state the Settings banner binds to.
 */

import { writable } from "svelte/store";
import type { UpdateStatus } from "../types";
import type { UpdateProgressEvent } from "../events";

export const updateStatus = writable<UpdateStatus | null>(null);

/** Download progress: [downloaded, total|null] or null when idle. */
export const updateProgress = writable<[number, number | null] | null>(null);

/** Replace update availability (from `update://status` or hydrate). */
export function applyUpdateStatus(next: UpdateStatus): void {
  updateStatus.set(next);
}

/** Apply a download-progress tick (from `update://progress`). */
export function applyUpdateProgress(e: UpdateProgressEvent): void {
  updateProgress.set([e.downloaded, e.total]);
}

/**
 * Drop any in-flight download progress. Used when a check/install fails or is
 * retried, so the banner falls back out of the downloading/installing states
 * instead of freezing on a stale percentage.
 */
export function clearUpdateProgress(): void {
  updateProgress.set(null);
}
