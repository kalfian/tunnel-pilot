import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { hydrateAll, subscribeEvents } from "./lib/hydrate";
import { initTheme } from "./lib/ui/theme";

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

const app = mount(App, { target });

export default app;
