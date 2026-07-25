/**
 * Command bus — UI-ephemeral requests that must be handled by a specific
 * screen's dialog surface (the ForwardForm and the delete-confirm live inside
 * ConnectionsView). The command palette and global shortcuts can be invoked
 * from anywhere, so they publish a request here; ConnectionsView consumes it
 * (on mount and reactively) and clears it. This keeps components dumb — the
 * palette never reaches into another view's local state.
 */

import { writable } from "svelte/store";
import type { ForwardConfig } from "../types";
import { activeView } from "../ui/view";

export type FormRequest =
  | { mode: "add" }
  | { mode: "edit"; forward: ForwardConfig };

/** Pending request to open the ForwardForm (add or edit). Null = none. */
export const pendingForm = writable<FormRequest | null>(null);

/** Pending request to open the delete-confirm dialog for a forward. */
export const pendingDelete = writable<ForwardConfig | null>(null);

/** Ask the Connections screen to open the Add-tunnel form. */
export function requestAddForm(): void {
  activeView.set("connections");
  pendingForm.set({ mode: "add" });
}

/** Ask the Connections screen to open the Edit form for a forward. */
export function requestEditForm(forward: ForwardConfig): void {
  activeView.set("connections");
  pendingForm.set({ mode: "edit", forward });
}

/** Ask the Connections screen to open the delete-confirm for a forward. */
export function requestDelete(forward: ForwardConfig): void {
  activeView.set("connections");
  pendingDelete.set(forward);
}
