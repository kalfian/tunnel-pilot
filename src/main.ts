import "./app.css";
import { mount } from "svelte";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App.svelte";
import TrayPopover from "./lib/components/TrayPopover.svelte";
import { hydrateAll, subscribeEvents } from "./lib/hydrate";
import { initTheme } from "./lib/ui/theme";

// The tray popover is the SAME index.html loaded into a separate webview window
// labelled `tray_popover` (contract with the Rust side). Detect it up front so
// we mount the compact panel instead of the full app.
let isTrayPopover = false;
try {
  isTrayPopover = getCurrentWebviewWindow().label === "tray_popover";
} catch {
  // Non-Tauri context (e.g. plain browser preview) → treat as the main app.
  isTrayPopover = false;
}
if (isTrayPopover) {
  // The popover window is transparent + rounded; drop the opaque canvas so the
  // panel's own surface/shadow reads as a floating menu.
  document.body.classList.add("popover");
}

// Boot (spec 02 §5 / AGENTS §5): the frontend owns no truth. Apply the theme
// from the settings store, subscribe every Rust→FE event to its reconciler,
// then pull a full snapshot. `subscribeEvents` MUST fully register (await)
// before the first hydrate: otherwise the two race and an event that fires
// during `app_hydrate` (before the listeners attach) is dropped, leaving a
// store stale until the next event (F45). Ordered: listeners first, then the
// snapshot establishes the baseline (last-write-wins — every reconciler is an
// idempotent set). `window://focus` re-hydrates on re-show (handled in hydrate).
initTheme();
async function boot(): Promise<void> {
  await subscribeEvents();
  await hydrateAll();
}
void boot();

const target = document.getElementById("app");
if (!target) {
  throw new Error("#app mount target not found");
}

const app = mount(isTrayPopover ? TrayPopover : App, { target });

export default app;
