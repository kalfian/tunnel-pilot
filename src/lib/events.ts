/**
 * Typed Rust→Frontend event subscriptions (spec 02 §7).
 *
 * Event names mirror the constants in `src-tauri/src/events.rs` exactly. Each
 * `onX` helper wraps Tauri's `listen<T>` with the correct payload type and
 * returns the `UnlistenFn`. Stores are the only place that reconcile these
 * payloads into UI state (AGENTS.md §5); M4 wires them up.
 */

import {
  listen,
  type EventCallback,
  type UnlistenFn,
} from "@tauri-apps/api/event";
import type {
  AppSettings,
  ForwardConfig,
  ForwardStatus,
  LogEntry,
  TunnelGroup,
  TunnelStats,
  UpdateStatus,
} from "./types";

export const EVENTS = {
  tunnelStatus: "tunnel://status",
  tunnelStats: "tunnel://stats",
  logLine: "log://line",
  logCleared: "log://cleared",
  forwardsChanged: "forwards://changed",
  groupsChanged: "groups://changed",
  settingsChanged: "settings://changed",
  updateStatus: "update://status",
  updateProgress: "update://progress",
  windowFocus: "window://focus",
  /** Emitted to the `tray_popover` webview on every open (re-hydrate trigger). */
  trayOpened: "tray://opened",
} as const;

// --- Payload types (mirror the Rust emit structs, spec 02 §7) ---

export interface TunnelStatusEvent {
  id: string;
  status: ForwardStatus;
  lastError: string | null;
}

export interface TunnelStatsEvent {
  id: string;
  stats: TunnelStats;
}

export interface UpdateProgressEvent {
  downloaded: number;
  total: number | null;
}

// --- Typed subscription helpers ---

export const onTunnelStatus = (
  cb: EventCallback<TunnelStatusEvent>,
): Promise<UnlistenFn> => listen(EVENTS.tunnelStatus, cb);

export const onTunnelStats = (
  cb: EventCallback<TunnelStatsEvent>,
): Promise<UnlistenFn> => listen(EVENTS.tunnelStats, cb);

export const onLogLine = (cb: EventCallback<LogEntry>): Promise<UnlistenFn> =>
  listen(EVENTS.logLine, cb);

export const onLogCleared = (cb: EventCallback<null>): Promise<UnlistenFn> =>
  listen(EVENTS.logCleared, cb);

export const onForwardsChanged = (
  cb: EventCallback<ForwardConfig[]>,
): Promise<UnlistenFn> => listen(EVENTS.forwardsChanged, cb);

export const onGroupsChanged = (
  cb: EventCallback<TunnelGroup[]>,
): Promise<UnlistenFn> => listen(EVENTS.groupsChanged, cb);

export const onSettingsChanged = (
  cb: EventCallback<AppSettings>,
): Promise<UnlistenFn> => listen(EVENTS.settingsChanged, cb);

export const onUpdateStatus = (
  cb: EventCallback<UpdateStatus>,
): Promise<UnlistenFn> => listen(EVENTS.updateStatus, cb);

export const onUpdateProgress = (
  cb: EventCallback<UpdateProgressEvent>,
): Promise<UnlistenFn> => listen(EVENTS.updateProgress, cb);

export const onWindowFocus = (cb: EventCallback<null>): Promise<UnlistenFn> =>
  listen(EVENTS.windowFocus, cb);

export const onTrayOpened = (cb: EventCallback<null>): Promise<UnlistenFn> =>
  listen(EVENTS.trayOpened, cb);
