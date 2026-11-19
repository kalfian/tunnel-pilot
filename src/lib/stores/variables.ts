/**
 * Variables store — mirror of Rust `AppState.variables` (spec 03 §24, 02 §5).
 * Reconciled from `variables://changed` and the `app_hydrate` snapshot (see
 * `lib/hydrate.ts`). Holds names, plain values and the `hasStoredValue` flag
 * of secret variables — never a secret value (the backend never sends one).
 */

import { derived, writable } from "svelte/store";
import type { Variable } from "../types";

export const variables = writable<Variable[]>([]);

/** Plain variables — the ones a username may reference as `{{NAME}}`. */
export const plainVariables = derived(variables, ($v) =>
  $v.filter((v) => !v.secret),
);

/** Secret variables — the ones a forward may use as its password. */
export const secretVariables = derived(variables, ($v) =>
  $v.filter((v) => v.secret),
);

/** Replace the list (from `variables://changed` or hydrate). */
export function applyVariables(list: Variable[] | undefined): void {
  // `?? []` tolerates an older backend whose snapshot has no `variables`.
  variables.set(list ?? []);
}
