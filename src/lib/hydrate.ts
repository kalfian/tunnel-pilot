/**
 * Store hydration + event wiring (AGENTS.md §5, spec 02 §5).
 *
 * The frontend holds no source of truth: on boot and on every window show it
 * must fully rehydrate from Rust via `app_hydrate()` before rendering live
 * data. This module is the ONE place that:
 *   1. pulls the `AppSnapshot` and fans it out into every store ({@link hydrateAll}), and
 *   2. subscribes the Rust→FE events to the stores' reconcile functions
 *      ({@link subscribeEvents}).
 *
 * `window://focus` (fired when a hidden window is re-shown, e.g. via
 * single-instance) triggers a fresh {@link hydrateAll} — never assume the
 * webview kept state while hidden. In web mode the same applies when the event
 * socket reconnects after a drop (`onEventsResumed`): events sent during the
 * gap are lost, so the snapshot is pulled again.
 */

import type { UnlistenFn } from "./transport";

import { appHydrate } from "./ipc";
import {
  onEventsResumed,
  onForwardsChanged,
  onGroupsChanged,
  onLogCleared,
  onLogLine,
  onSettingsChanged,
  onTunnelStats,
  onTunnelStatus,
  onTrayOpened,
  onUpdateProgress,
  onUpdateStatus,
  onWindowFocus,
} from "./events";

import {
  applyForwards,
  applyRuntimes,
  applyStats,
  applyStatus,
} from "./stores/forwards";
import { applyGroups } from "./stores/groups";
import { appendLogLine, resetLogs, setLogs } from "./stores/logs";
import { applySettings, setKeychainAvailable } from "./stores/settings";
import { applyUpdateProgress, applyUpdateStatus } from "./stores/updater";

/**
 * Fetch a full `AppSnapshot` and populate every store. Call on boot and on
 * `window://focus`. `applyForwards` runs before `applyRuntimes` so forwards
 * without a live runtime keep neutral defaults.
 */
export async function hydrateAll(): Promise<void> {
  const snap = await appHydrate();
  applyForwards(snap.forwards);
  applyRuntimes(snap.runtimes);
  applyGroups(snap.groups);
  applySettings(snap.settings);
  setLogs(snap.logs);
  applyUpdateStatus(snap.update);
  setKeychainAvailable(snap.keychainAvailable);
}

/**
 * Subscribe every Rust→FE event to its store reconcile function. Returns a
 * single `UnlistenFn` that tears down all subscriptions. Call once on boot
 * (after or alongside the first {@link hydrateAll}).
 */
export async function subscribeEvents(): Promise<UnlistenFn> {
  const unlisteners = await Promise.all([
    onTunnelStatus((e) => applyStatus(e.payload)),
    onTunnelStats((e) => applyStats(e.payload)),
    onLogLine((e) => appendLogLine(e.payload)),
    onLogCleared(() => resetLogs()),
    onForwardsChanged((e) => applyForwards(e.payload)),
    onGroupsChanged((e) => applyGroups(e.payload)),
    onSettingsChanged((e) => applySettings(e.payload)),
    onUpdateStatus((e) => applyUpdateStatus(e.payload)),
    onUpdateProgress((e) => applyUpdateProgress(e.payload)),
    onWindowFocus(() => {
      void hydrateAll();
    }),
    // The tray popover reuses this webview boot; on every open the backend
    // emits `tray://opened` so the panel re-pulls fresh state (spec: hydrate
    // on show).
    onTrayOpened(() => {
      void hydrateAll();
    }),
    // Web: the event socket reconnected after a gap — resync like a re-show.
    onEventsResumed(() => {
      void hydrateAll();
    }),
  ]);
  return () => {
    for (const unlisten of unlisteners) {
      unlisten();
    }
  };
}

/**
 * Boot the live mirror: subscribe first, then pull the snapshot (F45 ordering,
 * see main.ts). Returns the teardown. Web mode calls this after sign-in and
 * tears it down on sign-out, which also closes the event socket. A failed
 * first snapshot is reported, not fatal: the subscriptions stay up, and the
 * next reconnect or re-show pulls a fresh snapshot.
 */
export async function startSync(
  onHydrateError?: (err: unknown) => void,
): Promise<UnlistenFn> {
  const unlisten = await subscribeEvents();
  try {
    await hydrateAll();
  } catch (err) {
    onHydrateError?.(err);
  }
  return unlisten;
}
