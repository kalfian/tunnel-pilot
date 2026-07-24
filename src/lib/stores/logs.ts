/**
 * Logs store — mirror of the Rust ring buffer (cap 500, newest-first), fed via
 * `log://line` / `log://cleared` events (spec 02 §5, 03 §18).
 */

import { writable } from "svelte/store";
import type { LogEntry } from "../types";

export const logs = writable<LogEntry[]>([]);
