import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";
import { alias } from "./vite.config.ts";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "happy-dom",
    globals: true,
    // Vitest defaults to one worker per CPU. Each happy-dom worker holds a
    // few hundred MB, so cap the fan-out; several agents may run the suite
    // in parallel worktrees on one laptop.
    maxWorkers: 4,
    // Node 25+ ships its own global localStorage, which warns once per worker
    // when touched. Tests use happy-dom's storage, so turn Node's off.
    execArgv: ["--no-experimental-webstorage"],
    setupFiles: ["./vitest.setup.ts"],
    include: ["src/**/*.test.{ts,tsx}", "tests/**/*.test.{ts,tsx}"],
    exclude: ["**/node_modules/**", "**/dist/**", "tests/e2e/**"],
    coverage: {
      provider: "v8",
      reporter: ["text", "html", "lcov"],
      reportsDirectory: "./coverage",
      include: ["src/**/*.{ts,tsx}"],
      exclude: [
        "**/*.test.{ts,tsx}",
        "**/*.d.ts",
        "src/core/generated/**",
        "src/main.tsx",
      ],
    },
  },
  resolve: {
    alias,
    dedupe: ["react", "react-dom"],
  },
});
