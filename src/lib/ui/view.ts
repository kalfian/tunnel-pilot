/**
 * Active destination for the sidebar rail (spec 05 §2). Client-side view switch
 * (Connections / Activity / Settings) with a crossfade in App.svelte. Pure
 * UI-ephemeral state — not part of the Rust-backed data stores.
 */

import { writable } from "svelte/store";

export type ViewId = "connections" | "activity" | "settings";

export const activeView = writable<ViewId>("connections");
