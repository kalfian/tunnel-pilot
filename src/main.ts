import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";

// M0: mount the shell only. Event subscriptions (lib/events.ts) and the
// app_hydrate() boot call (per AGENTS.md §5 / spec 02 §5) are wired in M4 once
// the IPC commands exist. Keeping boot side-effect-free avoids ACL errors while
// the command surface is still stubbed.
const target = document.getElementById("app");
if (!target) {
  throw new Error("#app mount target not found");
}

const app = mount(App, { target });

export default app;
