/**
 * Forwards store — read-through mirror of Rust `AppState.configs` plus the
 * per-tunnel status/stats maps (spec 02 §5). Source of truth is Rust; these are
 * kept in sync by events (M4). Never authoritative on the frontend.
 */

import { writable } from "svelte/store";
import type { ForwardConfig, ForwardStatus, TunnelStats } from "../types";

export const forwards = writable<ForwardConfig[]>([]);
export const statusById = writable<Record<string, ForwardStatus>>({});
export const statsById = writable<Record<string, TunnelStats>>({});
export const lastErrorById = writable<Record<string, string | null>>({});
