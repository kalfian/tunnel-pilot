import tseslint from "typescript-eslint";
import svelte from "eslint-plugin-svelte";
import globals from "globals";

export default tseslint.config(
  // Ignore build output, deps, the Rust tree, and the untouched Flutter v1
  // sources (this config only governs the v2 Svelte/TS frontend).
  {
    ignores: [
      "dist/",
      "node_modules/",
      "src-tauri/",
      "lib/",
      "macos/",
      "windows/",
      "linux/",
      "build/",
      "test/",
      "assets/",
      "docs/",
      "spec/",
    ],
  },
  ...tseslint.configs.recommended,
  ...svelte.configs["flat/recommended"],
  {
    languageOptions: {
      globals: { ...globals.browser },
    },
  },
  {
    files: ["**/*.svelte", "**/*.svelte.ts"],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser,
      },
    },
  },
);
