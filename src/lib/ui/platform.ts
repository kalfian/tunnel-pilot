/**
 * Platform detection for window-chrome branching (spec 05 §3): macOS gets a
 * custom transparent titlebar + drag region; Windows/Linux use native OS
 * decorations and render no custom titlebar. We read the webview UA rather than
 * pulling in tauri-plugin-os — the OS string is stable in the Tauri webview and
 * this keeps the boot path free of an extra IPC round-trip.
 */

export type OS = "macos" | "windows" | "linux";

export function detectOS(): OS {
  const ua =
    typeof navigator !== "undefined" ? navigator.userAgent.toLowerCase() : "";
  if (ua.includes("mac")) return "macos";
  if (ua.includes("win")) return "windows";
  return "linux";
}

export const isMacOS = (): boolean => detectOS() === "macos";
