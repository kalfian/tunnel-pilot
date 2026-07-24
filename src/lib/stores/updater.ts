/**
 * Updater store — update availability + download progress (spec 02 §5).
 */

import { writable } from "svelte/store";
import type { UpdateStatus } from "../types";

export const updateStatus = writable<UpdateStatus | null>(null);
/** Download progress: [downloaded, total|null] or null when idle. */
export const updateProgress = writable<[number, number | null] | null>(null);
