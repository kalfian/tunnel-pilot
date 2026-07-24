/**
 * Settings store — mirror of Rust `settings_state` (spec 02 §5). `null` until
 * the first hydrate. Reconciled from `settings://changed` and the `app_hydrate`
 * snapshot (see `lib/hydrate.ts`).
 */

import { writable } from "svelte/store";
import type { AppSettings } from "../types";

export const settings = writable<AppSettings | null>(null);

/** True when keychain is unavailable and the plaintext fallback is in use. */
export const keychainUnavailable = writable<boolean>(false);

/** Replace the settings mirror (from `settings://changed` or hydrate). */
export function applySettings(next: AppSettings): void {
  settings.set(next);
}

/**
 * Set keychain availability from the hydrate snapshot's `keychainAvailable`.
 * The store holds the inverse (`keychainUnavailable`) so the UI binds a warning
 * directly.
 */
export function setKeychainAvailable(available: boolean): void {
  keychainUnavailable.set(!available);
}
