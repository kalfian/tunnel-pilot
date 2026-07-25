/**
 * Theme application (spec 05 §P19, design-tokens §1). The 3-way picker
 * (system/light/dark) lives in AppSettings.themeMode; `system` resolves to a
 * concrete `data-theme` on <html> here so tokens never depend on the media
 * query directly (no flash on toggle, honest picker). Driven by the settings
 * store — the component layer never writes data-theme itself.
 *
 * OLED mode (AppSettings.oledMode) adds `data-oled="true"` on <html> so the
 * `[data-theme="dark"][data-oled="true"]` token override in tokens.css kicks in.
 * It is set alongside the theme (pre-paint on boot, live on toggle). Since
 * `system` is already resolved to a concrete `data-theme` here, the CSS override
 * needs no media-query variant — system-dark carries `data-theme="dark"` too.
 */

import { writable, type Readable } from "svelte/store";
import type { ThemeMode } from "../types";
import { settings } from "../stores/settings";

const prefersDark = (): boolean =>
  typeof window !== "undefined" &&
  window.matchMedia("(prefers-color-scheme: dark)").matches;

function resolve(mode: ThemeMode): "light" | "dark" {
  if (mode === "system") return prefersDark() ? "dark" : "light";
  return mode;
}

/**
 * The concrete theme currently applied to the document ("light"/"dark"), with
 * `system` already resolved. Components read this to know the *effective* theme
 * (e.g. to disable OLED, which only has an effect in dark mode).
 */
const effectiveThemeStore = writable<"light" | "dark">("light");
export const effectiveTheme: Readable<"light" | "dark"> = effectiveThemeStore;

function apply(mode: ThemeMode, oled: boolean): void {
  const resolved = resolve(mode);
  const root = document.documentElement;
  root.setAttribute("data-theme", resolved);
  // OLED only means anything in dark mode; the CSS override is scoped to
  // [data-theme="dark"], so the attribute is harmless in light — but keep the
  // DOM honest and drop it when it can't apply.
  if (oled && resolved === "dark") {
    root.setAttribute("data-oled", "true");
  } else {
    root.removeAttribute("data-oled");
  }
  effectiveThemeStore.set(resolved);
}

/**
 * Start applying the theme + OLED override from the settings store, and follow
 * OS changes while in `system` mode. Returns a teardown fn. Call once on boot
 * (main.ts).
 */
export function initTheme(): () => void {
  let current: ThemeMode = "system";
  let oled = false;

  const unsubscribe = settings.subscribe((s) => {
    // Pre-hydrate the store is null — keep the light default until settings land.
    current = s?.themeMode ?? "system";
    oled = s?.oledMode ?? false;
    apply(current, oled);
  });

  const media = window.matchMedia("(prefers-color-scheme: dark)");
  const onSystemChange = (): void => {
    if (current === "system") apply(current, oled);
  };
  media.addEventListener("change", onSystemChange);

  return () => {
    unsubscribe();
    media.removeEventListener("change", onSystemChange);
  };
}
