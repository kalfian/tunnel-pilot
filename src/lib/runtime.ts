/**
 * Which host is running this UI (spec 02 §10).
 *
 * - Desktop: the Tauri webview. Tauri injects `window.__TAURI_INTERNALS__`
 *   before any page script runs, so the check is reliable at module load.
 * - Web: the same bundle served by `tunnel-pilot-cli daemon --web` to a plain
 *   browser. Commands go over HTTP, events over one WebSocket (`transport.ts`).
 *
 * This module has no imports on purpose: `transport.ts`, `platform.ts` and the
 * components all read it, and it must never pull Tauri code into the web
 * bundle. (OS detection for window chrome lives in `ui/platform.ts`.)
 */

export const isWeb: boolean =
  typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
