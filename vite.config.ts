import process from "node:process";
import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

// Path aliases. Keep in sync with tsconfig.json "paths" and vitest.config.ts.
export const alias = {
  "~": fileURLToPath(new URL("./src/app", import.meta.url)),
  "@core": fileURLToPath(new URL("./src/core", import.meta.url)),
  "@tests": fileURLToPath(new URL("./tests", import.meta.url)),
};

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss()],
  resolve: { alias },
  build: {
    // Local assets still cost WebKit startup time; keep a warning for bundle growth.
    chunkSizeWarningLimit: 1500,
    reportCompressedSize: false,
    rolldownOptions: {
      checks: {
        // Plugin timings are build profiling diagnostics, not app performance warnings.
        bundlerTimings: false,
      },
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available.
  //    Playwright overrides it with --port so e2e runs never collide with a
  //    running `tauri dev`.
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    ...(host ? { hmr: { protocol: "ws", host, port: 1421 } } : {}),
    watch: {
      // 3. tell Vite to ignore watching `src-tauri` and the Rust crates
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"],
    },
  },
}));
