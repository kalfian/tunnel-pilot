/**
 * Command palette store — open state, query, and a small recents ring
 * (all UI-ephemeral). The palette itself (Cmd/Ctrl+K fuzzy launcher) is the
 * marquee M5 feature (spec 05 §10). Recents let recently-run commands float to
 * the top; kept in memory only (cheap, not worth persisting).
 */

import { get, writable } from "svelte/store";

export const paletteOpen = writable<boolean>(false);
export const paletteQuery = writable<string>("");

/** Most-recently-run command ids, newest first (cap 8). */
export const paletteRecents = writable<string[]>([]);

const RECENTS_CAP = 8;

export function openPalette(): void {
  paletteQuery.set("");
  paletteOpen.set(true);
}

export function closePalette(): void {
  paletteOpen.set(false);
}

export function togglePalette(): void {
  if (get(paletteOpen)) closePalette();
  else openPalette();
}

/** Record a run command id so it floats up next time the palette opens. */
export function recordPaletteUse(id: string): void {
  paletteRecents.update((prev) => {
    const next = [id, ...prev.filter((x) => x !== id)];
    return next.length > RECENTS_CAP ? next.slice(0, RECENTS_CAP) : next;
  });
}
