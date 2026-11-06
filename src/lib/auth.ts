/**
 * Web-mode sign-in (spec 02 §10). The desktop app never imports this path's
 * UI and never calls these endpoints; they are not Tauri commands, so they
 * live here rather than in `ipc.ts`.
 *
 * `authPhase` drives the web shell: `checking` → (`signedOut` | `signedIn`).
 * A 401 or a socket close 4401 anywhere (see `transport.onUnauthorized`)
 * drops a signed-in shell back to `signedOut` with `sessionEnded` set, so the
 * login screen can say why it is showing.
 */

import { get, writable } from "svelte/store";
import { API_HEADERS, onUnauthorized, type UnlistenFn } from "./transport";
import type { WebSession } from "./types";

export type AuthPhase = "checking" | "signedOut" | "signedIn";

export const authPhase = writable<AuthPhase>("checking");

/** The login screen is showing because a live session ended (not a fresh visit). */
export const sessionEnded = writable(false);

/** `/api/session` could not be reached on the last check. */
export const serverUnreachable = writable(false);

/** Server build version from `/api/session` (web mode's "About"). */
export const serverVersion = writable<string | null>(null);

export type SignInResult =
  | { ok: true }
  | { ok: false; reason: "invalid" }
  | { ok: false; reason: "rateLimited"; retryAfterSec: number | null }
  | { ok: false; reason: "unreachable" }
  | { ok: false; reason: "server"; message: string };

/** Route every "session is gone" signal to the login screen. Call once. */
export function watchUnauthorized(): UnlistenFn {
  return onUnauthorized(() => {
    if (get(authPhase) === "signedIn") sessionEnded.set(true);
    authPhase.set("signedOut");
  });
}

/** `GET /api/session` → set the phase. Unreachable counts as signed out. */
export async function checkSession(): Promise<void> {
  try {
    const res = await fetch("/api/session", { credentials: "same-origin" });
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const body = (await res.json()) as WebSession;
    serverUnreachable.set(false);
    serverVersion.set(typeof body.version === "string" ? body.version : null);
    authPhase.set(body.authenticated === true ? "signedIn" : "signedOut");
  } catch {
    serverUnreachable.set(true);
    authPhase.set("signedOut");
  }
}

export async function signIn(password: string): Promise<SignInResult> {
  let res: Response;
  try {
    res = await fetch("/api/login", {
      method: "POST",
      credentials: "same-origin",
      headers: { "Content-Type": "application/json", ...API_HEADERS },
      body: JSON.stringify({ password }),
    });
  } catch {
    serverUnreachable.set(true);
    return { ok: false, reason: "unreachable" };
  }
  serverUnreachable.set(false);
  if (res.ok) {
    sessionEnded.set(false);
    authPhase.set("signedIn");
    // The version arrives with the session; refresh it without blocking.
    void refreshVersion();
    return { ok: true };
  }
  if (res.status === 401) return { ok: false, reason: "invalid" };
  if (res.status === 429) {
    return {
      ok: false,
      reason: "rateLimited",
      retryAfterSec: parseRetryAfter(res.headers.get("Retry-After")),
    };
  }
  return { ok: false, reason: "server", message: await errorText(res) };
}

/**
 * `POST /api/logout`, then show the login screen. Throws when the server
 * cannot be reached: the cookie would still be valid, so pretending to have
 * signed out would be a lie.
 */
export async function signOut(): Promise<void> {
  let res: Response;
  try {
    res = await fetch("/api/logout", {
      method: "POST",
      credentials: "same-origin",
      headers: API_HEADERS,
    });
  } catch {
    throw new Error("can't reach the server");
  }
  if (!res.ok && res.status !== 401) {
    throw new Error(await errorText(res));
  }
  sessionEnded.set(false);
  authPhase.set("signedOut");
}

async function refreshVersion(): Promise<void> {
  try {
    const res = await fetch("/api/session", { credentials: "same-origin" });
    if (!res.ok) return;
    const body = (await res.json()) as WebSession;
    if (typeof body.version === "string") serverVersion.set(body.version);
  } catch {
    // Cosmetic only.
  }
}

function parseRetryAfter(value: string | null): number | null {
  if (!value) return null;
  const seconds = Number(value);
  if (Number.isFinite(seconds) && seconds > 0) return Math.ceil(seconds);
  const at = Date.parse(value);
  if (Number.isNaN(at)) return null;
  const delta = Math.ceil((at - Date.now()) / 1000);
  return delta > 0 ? delta : null;
}

async function errorText(res: Response): Promise<string> {
  try {
    const body = (await res.json()) as { message?: unknown };
    if (typeof body.message === "string" && body.message.trim() !== "") {
      return body.message;
    }
  } catch {
    // Not JSON.
  }
  return `HTTP ${res.status}`;
}
