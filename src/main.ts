import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { hydrateAll, subscribeEvents } from "./lib/hydrate";
import { initTheme } from "./lib/ui/theme";

// Boot (spec 02 §5 / AGENTS §5): the frontend owns no truth. Apply the theme
// from the settings store, subscribe every Rust→FE event to its reconciler,
// then pull a full snapshot. `subscribeEvents` starts before the first hydrate
// so a status/log event that fires mid-hydrate isn't dropped; the snapshot then
// establishes the baseline (last-write-wins is correct — every reconciler is an
// idempotent set). `window://focus` re-hydrates on re-show (handled in hydrate).
initTheme();
void subscribeEvents();
void hydrateAll();

const target = document.getElementById("app");
if (!target) {
  throw new Error("#app mount target not found");
}

const app = mount(App, { target });

export default app;
