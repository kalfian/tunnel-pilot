import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { svelteTesting } from "@testing-library/svelte/vite";

// Two kinds of frontend tests share one runner:
//   - Store/helper unit tests run in Node (pure TS + Svelte stores, mock IPC).
//   - Component (.svelte) tests opt into jsdom per-file via the
//     `// @vitest-environment jsdom` docblock and render with
//     @testing-library/svelte. The Svelte plugin compiles components for both;
//     `svelteTesting()` wires browser resolution + auto-cleanup.
export default defineConfig({
  plugins: [svelte({ hot: false }), svelteTesting()],
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
