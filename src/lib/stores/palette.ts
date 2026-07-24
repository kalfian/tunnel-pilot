/**
 * Command palette store — open state + query (UI-ephemeral). The palette
 * (Cmd/Ctrl+K fuzzy search) lands in M5 (spec 02 §4, 07 M5).
 */

import { writable } from "svelte/store";

export const paletteOpen = writable<boolean>(false);
export const paletteQuery = writable<string>("");
