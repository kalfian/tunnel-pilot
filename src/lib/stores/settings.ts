/**
 * Settings store — mirror of Rust `settings_state` (spec 02 §5). `null` until
 * the first hydrate.
 */

import { writable } from "svelte/store";
import type { AppSettings } from "../types";

export const settings = writable<AppSettings | null>(null);
/** True when keychain is unavailable and the plaintext fallback is in use. */
export const keychainUnavailable = writable<boolean>(false);
