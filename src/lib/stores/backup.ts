/**
 * Backup store — the import mode the UI holds while the user picks how a backup
 * should be applied (spec 02 §6.5). Pure UI-ephemeral state; the actual
 * `import_backup(path, mode)` call goes through `lib/ipc.ts`. Default `merge`
 * (non-destructive).
 */

import { writable } from "svelte/store";
import type { ImportMode } from "../types";

export const importMode = writable<ImportMode>("merge");
