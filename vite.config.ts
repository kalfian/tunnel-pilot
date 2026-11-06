import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed dev-server port; it fails if that port is unavailable.
const host = process.env.TAURI_DEV_HOST;

// Web mode (spec 02 §10): `vite build --mode web` → `dist-web/`, the bundle
// `tunnel-pilot-cli` embeds and serves at `/`. The same UI picks its transport
// at runtime, so the only build differences are the output dir and, for
// `vite --mode web`, a dev proxy to a running server.
const WEB_DEV_TARGET =
  process.env.TUNNEL_PILOT_WEB_ORIGIN ?? "http://127.0.0.1:1855";

// https://vitejs.dev/config/
export default defineConfig(({ mode }) => {
  const web = mode === "web";
  return {
    plugins: [svelte()],
    base: "/",

    // Prevent Vite from obscuring Rust errors.
    clearScreen: false,
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
      watch: {
        // Don't watch the Rust source tree (avoids reload storms during builds).
        ignored: ["**/src-tauri/**"],
      },
      // `pnpm dev:web`: forward the API + event socket to a running
      // `tunnel-pilot-cli daemon --web`. The server checks the socket's Origin
      // against its Host, so present the target's own origin.
      proxy: web
        ? {
            "/api": {
              target: WEB_DEV_TARGET,
              changeOrigin: true,
              ws: true,
              configure: (proxy) => {
                proxy.on("proxyReqWs", (req) => {
                  req.setHeader("origin", WEB_DEV_TARGET);
                });
              },
            },
          }
        : undefined,
    },

    build: {
      outDir: web ? "dist-web" : "dist",
      target: "es2021",
      minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
      sourcemap: !!process.env.TAURI_ENV_DEBUG,
    },
  };
});
