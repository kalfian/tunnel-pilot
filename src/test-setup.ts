/**
 * Vitest setup. `runtime.ts` treats a page without `__TAURI_INTERNALS__` as web
 * mode; jsdom is such a page, so component suites would silently test the web
 * variant. Mark jsdom as the desktop webview by default. Web-mode suites opt in
 * by mocking `./runtime` (`vi.mock(".../runtime", () => ({ isWeb: true }))`).
 */

if (typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window)) {
  Object.defineProperty(window, "__TAURI_INTERNALS__", {
    value: {},
    configurable: true,
  });
}
