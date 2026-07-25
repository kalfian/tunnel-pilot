// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { get } from "svelte/store";
import type { AppSettings } from "../types";
import { settings } from "../stores/settings";
import { initTheme, effectiveTheme } from "./theme";

// jsdom has no matchMedia — install a controllable mock. Only the
// prefers-color-scheme:dark query is consulted (theme.ts `resolve`).
const mediaListeners: Array<() => void> = [];
function installMatchMedia(systemDark: boolean): void {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: systemDark && query.includes("dark"),
    media: query,
    onchange: null,
    addEventListener: (_: string, cb: () => void) => mediaListeners.push(cb),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: () => false,
  }));
}

const BASE: AppSettings = {
  launchAtLogin: false,
  showNotifications: true,
  themeMode: "dark",
  oledMode: false,
  autoReconnect: true,
  autoReconnectDelaySec: 3,
  autoReconnectMaxRetries: 5,
  showInDock: true,
  autoCheckUpdates: true,
  lastSkippedVersion: null,
};

const root = document.documentElement;

describe("theme.ts — OLED (data-oled) application", () => {
  let teardown: () => void = () => {};

  beforeEach(() => {
    mediaListeners.length = 0;
    installMatchMedia(false);
    settings.set(null);
    root.removeAttribute("data-theme");
    root.removeAttribute("data-oled");
  });

  afterEach(() => {
    teardown();
    teardown = () => {};
  });

  it("adds data-oled='true' when dark + oledMode, removes it when off", () => {
    teardown = initTheme();

    settings.set({ ...BASE, themeMode: "dark", oledMode: true });
    expect(root.getAttribute("data-theme")).toBe("dark");
    expect(root.getAttribute("data-oled")).toBe("true");

    // Toggling OLED off live removes the attribute (no restart).
    settings.set({ ...BASE, themeMode: "dark", oledMode: false });
    expect(root.getAttribute("data-theme")).toBe("dark");
    expect(root.hasAttribute("data-oled")).toBe(false);
  });

  it("does NOT set data-oled in light mode even when oledMode is on", () => {
    teardown = initTheme();
    settings.set({ ...BASE, themeMode: "light", oledMode: true });
    expect(root.getAttribute("data-theme")).toBe("light");
    expect(root.hasAttribute("data-oled")).toBe(false);
  });

  it("resolves system→dark and applies OLED when the OS is dark", () => {
    installMatchMedia(true);
    teardown = initTheme();
    settings.set({ ...BASE, themeMode: "system", oledMode: true });
    expect(root.getAttribute("data-theme")).toBe("dark");
    expect(root.getAttribute("data-oled")).toBe("true");
    expect(get(effectiveTheme)).toBe("dark");
  });

  it("drops data-oled when switching from dark to light with OLED still on", () => {
    teardown = initTheme();
    settings.set({ ...BASE, themeMode: "dark", oledMode: true });
    expect(root.getAttribute("data-oled")).toBe("true");
    settings.set({ ...BASE, themeMode: "light", oledMode: true });
    expect(root.hasAttribute("data-oled")).toBe(false);
    expect(get(effectiveTheme)).toBe("light");
  });
});
