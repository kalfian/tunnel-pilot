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

/**
 * Replace update availability (from `update://status` or hydrate). Carries the
 * whole status, including `error` — a user-initiated check failure now arrives
 * as `status.error` on this event (the backend no longer throws for checks), so
 * the Settings banner derives its error state from the store, not a rejection.
 */
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

/**
 * Coerce an unknown update failure into a human-readable string.
 *
 * Update errors reach the UI as an IPC rejection (a serialized Rust `AppError`)
 * or, historically, an `update://status` failure. The shape is not guaranteed:
 * it may be a bare `string`, an `Error`, a structured `{ message }` object, or
 * something opaque. Coerce safely so an object can NEVER render as the literal
 * "[object Object]".
 *
 * Returns an empty string when there is no meaningful message to show — callers
 * treat that as "no error" and keep the banner idle (a silent/benign failure,
 * e.g. a startup check with no release yet, must not raise a scary red banner).
 */
export function toUpdateErrorMessage(e: unknown): string {
  if (e == null) return "";
  if (typeof e === "string") return e.trim();
  if (e instanceof Error) return e.message.trim();
  if (typeof e === "object") {
    const message = (e as { message?: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") {
      return message.trim();
    }
    try {
      const json = JSON.stringify(e);
      // `{}` carries no signal — treat as no message rather than showing braces.
      return json === "{}" ? "" : json;
    } catch {
      return "";
    }
  }
  return String(e).trim();
}
