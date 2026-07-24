/**
 * Theme application (spec 05 §P19, design-tokens §1). The 3-way picker
 * (system/light/dark) lives in AppSettings.themeMode; `system` resolves to a
 * concrete `data-theme` on <html> here so tokens never depend on the media
 * query directly (no flash on toggle, honest picker). Driven by the settings
 * store — the component layer never writes data-theme itself.
 */

import type { ThemeMode } from "../types";
import { settings } from "../stores/settings";

const prefersDark = (): boolean =>
  typeof window !== "undefined" &&
  window.matchMedia("(prefers-color-scheme: dark)").matches;

function resolve(mode: ThemeMode): "light" | "dark" {
  if (mode === "system") return prefersDark() ? "dark" : "light";
  return mode;
}

function apply(mode: ThemeMode): void {
  document.documentElement.setAttribute("data-theme", resolve(mode));
}

/**
 * Start applying the theme from the settings store + follow OS changes while in
 * `system` mode. Returns a teardown fn. Call once on boot (main.ts).
 */
export function initTheme(): () => void {
  let current: ThemeMode = "system";

  const unsubscribe = settings.subscribe((s) => {
    // Pre-hydrate the store is null — keep the light default until settings land.
    current = s?.themeMode ?? "system";
    apply(current);
  });

  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const onSystemChange = (): void => {
    if (current === "system") apply(current);
  };
  media.addEventListener("change", onSystemChange);

  return () => {
    unsubscribe();
    media.removeEventListener("change", onSystemChange);
  };
}
