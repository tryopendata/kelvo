import { execFileSync } from "node:child_process";
import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright E2E Test Configuration
 *
 * Runs against the Vite dev server in a browser, fed by the mock transport.
 * The packaged app's WKWebView is not reachable from Playwright
 * (tauri-driver/WebDriver does not support it on macOS); Rust integration
 * tests plus scripted manual checks cover that layer. E2E here is for
 * screenshots of each screen in both themes, contrast checks, and
 * cross-route flows.
 *
 * Run tests:
 *   bun run test:e2e
 *
 * Run tests with UI:
 *   bun run test:e2e:ui
 *
 * Ports. Every run starts its own dev server on a port the OS hands out as
 * free, and never adopts a server that is already listening. `tauri dev` holds
 * 1420, and several worktrees may run e2e at once; with `reuseExistingServer`
 * Playwright would adopt whichever stale server was there and test the wrong
 * code.
 *
 * - Default: a free port per run, no reuse.
 * - PLAYWRIGHT_PORT=<n>: fixed port n, no reuse. If it is taken, Playwright
 *   fails fast with "is already used".
 * - PLAYWRIGHT_REUSE_SERVER=1: local opt-in to iterate against a dev server
 *   you started yourself (PLAYWRIGHT_PORT, default 5173). It must be running
 *   with VITE_TRANSPORT=mock.
 */
const REUSE_SERVER = process.env.PLAYWRIGHT_REUSE_SERVER === "1";

// A bare listen(0) binds the dual-stack wildcard, which covers both 127.0.0.1
// and ::1 where Vite's "localhost" may land.
const FREE_PORT_SCRIPT = `
const net = require("net");
const s = net.createServer();
s.on("error", (e) => { throw e; });
s.listen(0, () => { process.stdout.write(String(s.address().port)); s.close(); });
`;

function resolvePort(): number {
  if (process.env.PLAYWRIGHT_PORT || REUSE_SERVER) {
    return Number(process.env.PLAYWRIGHT_PORT ?? 5173);
  }
  // The config is evaluated again in every worker process. Workers inherit the
  // runner's env, so the port picked here is stored there and reused rather
  // than picked again.
  if (!process.env.PLAYWRIGHT_RUN_PORT) {
    process.env.PLAYWRIGHT_RUN_PORT = execFileSync(
      process.execPath,
      ["-e", FREE_PORT_SCRIPT],
      { encoding: "utf8" }
    ).trim();
  }
  const port = Number(process.env.PLAYWRIGHT_RUN_PORT);
  if (!Number.isInteger(port) || port <= 0) {
    throw new Error(
      `playwright.config: bad PLAYWRIGHT_RUN_PORT "${process.env.PLAYWRIGHT_RUN_PORT}"`
    );
  }
  return port;
}

const PORT = resolvePort();
const BASE_URL = `http://localhost:${PORT}`;

export default defineConfig({
  testDir: "./tests/e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  // One browser per core oversubscribes, and screenshot specs are sensitive
  // to a starved renderer. Half the cores locally, two on CI.
  workers: process.env.CI ? 2 : "50%",
  // "line" makes failures visible on stdout (agents read that); html's open
  // default is "on-failure", which would block a headless run on a report
  // server, so keep it "never".
  reporter: [["line"], ["html", { open: "never" }]],
  timeout: 30000,
  globalTimeout: process.env.CI ? 10 * 60 * 1000 : 0,

  use: {
    baseURL: BASE_URL,
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
    {
      // The app ships in WKWebView; WebKit is the closer engine for
      // screenshot comparison.
      name: "webkit",
      use: { ...devices["Desktop Safari"] },
    },
  ],

  webServer: {
    command: `bun run dev --port ${PORT} --strictPort`,
    url: BASE_URL,
    reuseExistingServer: REUSE_SERVER,
    timeout: 120000,
    stdout: "pipe",
    stderr: "pipe",
    env: {
      ...process.env,
      // Tells the app to build the mock transport instead of the Tauri
      // Channel one. Vite bakes import.meta.env at server start.
      VITE_TRANSPORT: "mock",
    },
  },
});
