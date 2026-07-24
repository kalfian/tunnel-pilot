/**
 * Groups store — mirror of Rust `AppState.groups` + the active tag filter
 * (UI-ephemeral) (spec 02 §5).
 */

import { writable } from "svelte/store";
import type { TunnelGroup } from "../types";

export const groups = writable<TunnelGroup[]>([]);
/** Active tag filter (null = show all). UI-only ephemeral state. */
export const activeTag = writable<string | null>(null);
