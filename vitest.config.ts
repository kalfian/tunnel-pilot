import { defineConfig } from "vitest/config";

// Store/helper unit tests run in a plain Node environment — they exercise pure
// TS + Svelte stores and mock the IPC/event layer (`lib/ipc.ts`,
// `lib/events.ts`), so no jsdom or Tauri runtime is needed. Component (.svelte)
// tests land with the ui-ux work and can add their own jsdom config then.
export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
