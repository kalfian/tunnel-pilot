/**
 * Toast queue (spec 05 §11, §4.3) — transient confirmations ("Copied"),
 * errors, and undo affordances. UI-ephemeral; any component pushes via
 * `pushToast`, the singleton `<ToastHost>` in App.svelte renders the stack.
 */

import { writable } from "svelte/store";

export type ToastTone = "info" | "success" | "error";

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Toast {
  id: number;
  message: string;
  tone: ToastTone;
  action?: ToastAction;
  /** ms before auto-dismiss; 0 = sticky (user must act, e.g. error). */
  duration: number;
}

export const toasts = writable<Toast[]>([]);

let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

export function dismissToast(id: number): void {
  const timer = timers.get(id);
  if (timer) {
    clearTimeout(timer);
    timers.delete(id);
  }
  toasts.update((list) => list.filter((t) => t.id !== id));
}

export function pushToast(
  message: string,
  opts: { tone?: ToastTone; action?: ToastAction; duration?: number } = {},
): number {
  const id = nextId++;
  const tone = opts.tone ?? "info";
  const duration = opts.duration ?? (tone === "error" ? 6000 : 2600);
  toasts.update((list) => [
    ...list,
    { id, message, tone, action: opts.action, duration },
  ]);
  if (duration > 0) {
    timers.set(
      id,
      setTimeout(() => dismissToast(id), duration),
    );
  }
  return id;
}
