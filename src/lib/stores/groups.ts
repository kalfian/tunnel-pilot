/**
 * Groups store — mirror of Rust `AppState.groups` + the active tag filter
 * (UI-ephemeral) (spec 02 §5). Reconciled from `groups://changed` and the
 * `app_hydrate` snapshot (see `lib/hydrate.ts`). The groups UI itself is M5;
 * the data can exist now.
 */

import { writable } from "svelte/store";
import type { TunnelGroup } from "../types";

export const groups = writable<TunnelGroup[]>([]);

/** Active tag filter (null = show all). UI-only ephemeral state. */
export const activeTag = writable<string | null>(null);

/** Replace the groups list (from `groups://changed` or hydrate). */
export function applyGroups(list: TunnelGroup[]): void {
  groups.set(list);
}
