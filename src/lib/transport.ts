/**
 * Transport — the single seam between the typed wrappers (`ipc.ts`,
 * `events.ts`) and whatever hosts the UI (spec 02 §10).
 *
 * `invoke` and `listen` keep the exact shapes of `@tauri-apps/api` so the
 * wrappers do not care which host they run on:
 *
 * - Desktop (Tauri webview): delegate to `@tauri-apps/api`. The import is
 *   dynamic, so the web build never loads Tauri code at runtime.
 * - Web (`tunnel-pilot-cli daemon --web`):
 *     commands → `POST /api/invoke/<command>` with the args object as JSON;
 *     events   → ONE shared WebSocket to `/api/events`, frames
 *                `{ event, payload }` fanned out to the registered listeners.
 *   The socket reconnects with capped exponential backoff. After any gap it
 *   tells {@link onReconnect} subscribers, which re-run `app_hydrate` (the
 *   same path a desktop window re-show takes), because events sent while the
 *   socket was down are gone.
 *
 * A 401 from a command, or the server closing the socket with 4401 (session
 * ended), is reported to {@link onUnauthorized} subscribers; the auth layer
 * then returns to the login screen.
 */

import { isWeb } from "./runtime";
import type { AppError, AppErrorKind } from "./types";

export type UnlistenFn = () => void;

/** Same shape as Tauri's `Event<T>`. */
export interface TransportEvent<T> {
  event: string;
  id: number;
  payload: T;
}

export type EventCallback<T> = (event: TransportEvent<T>) => void;

export type InvokeArgs = Record<string, unknown>;

/** CSRF guard: every non-GET `/api/*` request carries it (spec 02 §10). */
export const API_HEADERS: Readonly<Record<string, string>> = {
  "X-Tunnel-Pilot": "1",
};

/** WebSocket close code the server uses when the session ends. */
export const WS_SESSION_ENDED = 4401;

const RECONNECT_BASE_MS = 500;
const RECONNECT_MAX_MS = 10_000;
/** `listen` waits at most this long for the socket before resolving anyway. */
const READY_TIMEOUT_MS = 5_000;
const WS_OPEN = 1;

// --- Errors ---

const ERROR_KINDS: ReadonlySet<string> = new Set<AppErrorKind>([
  "ssh",
  "connection",
  "notFound",
  "invalidInput",
  "config",
  "storage",
  "backup",
  "credential",
  "updater",
  "serialization",
  "io",
  "internal",
  "unauthorized",
  "hostKeyMismatch",
]);

/**
 * Build an `AppError` with the same enumerable shape Tauri rejects with
 * (`{ kind, message }`), plus a non-enumerable `toString` so the many
 * `String(err)` toasts read as the message instead of "[object Object]".
 */
export function appError(kind: AppErrorKind, message: string): AppError {
  const err: AppError = { kind, message };
  Object.defineProperty(err, "toString", {
    value: () => message,
    enumerable: false,
  });
  return err;
}

/** Normalize a Tauri rejection into an `AppError` (unknown shapes pass through). */
function toAppError(err: unknown): unknown {
  if (err && typeof err === "object") {
    const { kind, message } = err as { kind?: unknown; message?: unknown };
    if (typeof kind === "string" && typeof message === "string") {
      return appError(
        ERROR_KINDS.has(kind) ? (kind as AppErrorKind) : "internal",
        message,
      );
    }
  }
  return err;
}

async function readError(res: Response): Promise<AppError> {
  let body: unknown = null;
  try {
    body = await res.json();
  } catch {
    body = null;
  }
  if (body && typeof body === "object") {
    const { kind, message } = body as { kind?: unknown; message?: unknown };
    if (typeof kind === "string" && typeof message === "string") {
      return appError(
        ERROR_KINDS.has(kind) ? (kind as AppErrorKind) : "internal",
        message,
      );
    }
  }
  if (res.status === 401) return appError("unauthorized", "Not signed in");
  if (res.status === 404) return appError("notFound", "Not available here");
  return appError("internal", `Server error (HTTP ${res.status})`);
}

// --- Auth signal ---

const unauthorizedHandlers = new Set<() => void>();

/** Subscribe to "the session is gone" (HTTP 401 or socket close 4401). */
export function onUnauthorized(cb: () => void): UnlistenFn {
  unauthorizedHandlers.add(cb);
  return () => {
    unauthorizedHandlers.delete(cb);
  };
}

function notifyUnauthorized(): void {
  for (const cb of [...unauthorizedHandlers]) cb();
}

// --- invoke ---

type TauriCore = typeof import("@tauri-apps/api/core");
type TauriEvent = typeof import("@tauri-apps/api/event");
let tauriCore: Promise<TauriCore> | null = null;
let tauriEvent: Promise<TauriEvent> | null = null;

/** Run a backend command. Rejects with an `AppError` (`{ kind, message }`). */
export async function invoke<T>(
  command: string,
  args?: InvokeArgs,
): Promise<T> {
  if (!isWeb) {
    tauriCore ??= import("@tauri-apps/api/core");
    const core = await tauriCore;
    try {
      return await core.invoke<T>(command, args);
    } catch (err) {
      // Tauri rejects with the plain serialized `AppError`; give it the same
      // readable `toString` the web transport's errors have, so toasts show the
      // message on both products instead of "[object Object]".
      throw toAppError(err);
    }
  }
  return httpInvoke<T>(command, args);
}

async function httpInvoke<T>(command: string, args?: InvokeArgs): Promise<T> {
  let res: Response;
  try {
    res = await fetch(`/api/invoke/${encodeURIComponent(command)}`, {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json", ...API_HEADERS },
      body: JSON.stringify(args ?? {}),
    });
  } catch {
    throw appError("io", "Can't reach the Tunnel Pilot server");
  }
  if (!res.ok) {
    const err = await readError(res);
    if (res.status === 401) notifyUnauthorized();
    throw err;
  }
  const text = await res.text();
  if (text === "") return null as T;
  try {
    return JSON.parse(text) as T;
  } catch {
    throw appError("serialization", "The server sent an unreadable reply");
  }
}

// --- listen (web: one shared WebSocket) ---

const listeners = new Map<string, Set<EventCallback<unknown>>>();
const reconnectHandlers = new Set<() => void>();

let socket: WebSocket | null = null;
let retryTimer: ReturnType<typeof setTimeout> | null = null;
let probing = false;
let attempt = 0;
let eventSeq = 0;
/** A socket closed or failed since the last open: events may have been lost. */
let missedEvents = false;
let readyWaiters: (() => void)[] = [];
let onlineHooked = false;

/**
 * Subscribe to "the event stream came back after a gap". Web only — on the
 * desktop the Tauri event bus never drops, so this never fires there.
 */
export function onReconnect(cb: () => void): UnlistenFn {
  reconnectHandlers.add(cb);
  return () => {
    reconnectHandlers.delete(cb);
  };
}

/** Subscribe to a backend event. Resolves once the subscription is live. */
export async function listen<T>(
  event: string,
  cb: EventCallback<T>,
): Promise<UnlistenFn> {
  if (!isWeb) {
    tauriEvent ??= import("@tauri-apps/api/event");
    const mod = await tauriEvent;
    return mod.listen<T>(event, cb);
  }

  const set = listeners.get(event) ?? new Set<EventCallback<unknown>>();
  const entry = cb as EventCallback<unknown>;
  set.add(entry);
  listeners.set(event, set);
  ensureSocket();
  await whenSettled();

  let active = true;
  return () => {
    if (!active) return;
    active = false;
    const current = listeners.get(event);
    current?.delete(entry);
    if (current && current.size === 0) listeners.delete(event);
    if (listeners.size === 0) closeSocket();
  };
}

function eventsUrl(): string {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${location.host}/api/events`;
}

function ensureSocket(): void {
  if (socket || retryTimer || probing) return;
  if (!onlineHooked) {
    onlineHooked = true;
    // Back online: skip the rest of the backoff wait.
    window.addEventListener("online", () => {
      if (retryTimer && listeners.size > 0) {
        clearTimeout(retryTimer);
        retryTimer = null;
        attempt = 0;
        connect();
      }
    });
  }
  connect();
}

/** Resolves when the current socket opens or fails (bounded wait). */
function whenSettled(): Promise<void> {
  if (!socket || socket.readyState === WS_OPEN) return Promise.resolve();
  return new Promise((resolve) => {
    const timer = setTimeout(done, READY_TIMEOUT_MS);
    function done(): void {
      clearTimeout(timer);
      readyWaiters = readyWaiters.filter((w) => w !== done);
      resolve();
    }
    readyWaiters.push(done);
  });
}

function settleWaiters(): void {
  const waiters = readyWaiters;
  readyWaiters = [];
  for (const w of waiters) w();
}

function connect(): void {
  const ws = new WebSocket(eventsUrl());
  socket = ws;
  let opened = false;

  ws.onopen = () => {
    if (socket !== ws) return;
    opened = true;
    attempt = 0;
    settleWaiters();
    if (missedEvents) {
      missedEvents = false;
      for (const cb of [...reconnectHandlers]) cb();
    }
  };
  ws.onmessage = (msg: MessageEvent) => {
    if (socket === ws) dispatch(msg.data);
  };
  ws.onclose = (e: CloseEvent) => {
    if (socket !== ws) return;
    socket = null;
    settleWaiters();
    if (listeners.size === 0) return;
    missedEvents = true;
    if (e.code === WS_SESSION_ENDED) {
      notifyUnauthorized();
      return;
    }
    if (opened) {
      scheduleReconnect();
    } else {
      // The handshake itself failed. A browser hides the HTTP status of a
      // refused upgrade, so ask whether the session is still valid before
      // retrying forever against an expired cookie.
      void probeThenReconnect();
    }
  };
}

async function probeThenReconnect(): Promise<void> {
  probing = true;
  let signedOut = false;
  try {
    const res = await fetch("/api/session", { credentials: "same-origin" });
    if (res.ok) {
      const body = (await res.json()) as { authenticated?: unknown };
      signedOut = body.authenticated === false;
    }
  } catch {
    // Server unreachable: keep retrying with backoff.
  } finally {
    probing = false;
  }
  if (listeners.size === 0) return;
  if (signedOut) {
    notifyUnauthorized();
    return;
  }
  scheduleReconnect();
}

function scheduleReconnect(): void {
  if (retryTimer) return;
  const delay = Math.min(RECONNECT_MAX_MS, RECONNECT_BASE_MS * 2 ** attempt);
  attempt += 1;
  retryTimer = setTimeout(() => {
    retryTimer = null;
    if (listeners.size > 0 && !socket) connect();
  }, delay);
}

function closeSocket(): void {
  if (retryTimer) {
    clearTimeout(retryTimer);
    retryTimer = null;
  }
  const ws = socket;
  socket = null;
  attempt = 0;
  missedEvents = false;
  settleWaiters();
  if (ws && ws.readyState <= WS_OPEN) ws.close(1000, "unsubscribed");
}

function dispatch(raw: unknown): void {
  if (typeof raw !== "string") return;
  let frame: unknown;
  try {
    frame = JSON.parse(raw);
  } catch {
    return;
  }
  if (!frame || typeof frame !== "object") return;
  const { event, payload } = frame as { event?: unknown; payload?: unknown };
  if (typeof event !== "string") return;
  const set = listeners.get(event);
  if (!set) return;
  eventSeq += 1;
  const evt: TransportEvent<unknown> = {
    event,
    id: eventSeq,
    payload: payload ?? null,
  };
  for (const cb of [...set]) {
    try {
      cb(evt);
    } catch (err) {
      console.error(`listener for ${event} failed`, err);
    }
  }
}
