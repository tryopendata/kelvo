import { execFileSync } from "node:child_process";
import { readFileSync, rmSync } from "node:fs";
import path from "node:path";
import { expect, type Page, test } from "@playwright/test";

/**
 * The production bundle under the CSP that `tauri.conf.json` ships. The app
 * webview is not reachable from Playwright, so this builds the frontend,
 * serves it from a fake origin with the policy as a response header (as
 * Tauri's asset protocol does), and walks the windows and the parts that
 * inject styles at runtime (Radix dialog and select, sonner). Outside Tauri
 * the bundle falls back to the mock transport, so the IPC directives are not
 * exercised here; `bun run tauri dev` does not apply the CSP at all on
 * desktop (Tauri only sets it on bundled assets), so the real check is a
 * built app.
 */

const ROOT = path.resolve(import.meta.dirname, "../..");
const ORIGIN = "http://kelvo.csp";

const MIME: Record<string, string> = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
  ".png": "image/png",
};

/** The directive map from tauri.conf.json as one header value. */
function shippedCsp(): string {
  const conf = JSON.parse(
    readFileSync(path.join(ROOT, "src-tauri/tauri.conf.json"), "utf8")
  ) as { app: { security: { csp: Record<string, string> | null } } };
  const csp = conf.app.security.csp;
  if (!csp) throw new Error("tauri.conf.json ships no CSP");
  return Object.entries(csp)
    .map(([k, v]) => `${k} ${v}`)
    .join("; ");
}

test.describe.configure({ mode: "serial" });

let dist = "";

test.beforeAll(({ browserName }) => {
  dist = path.join(ROOT, "test-results", `csp-dist-${browserName}`);
  rmSync(dist, { recursive: true, force: true });
  execFileSync(
    path.join(ROOT, "node_modules/.bin/vite"),
    ["build", "--outDir", dist, "--emptyOutDir", "--logLevel", "error"],
    { cwd: ROOT, stdio: "pipe", env: { ...process.env, VITE_TRANSPORT: "" } }
  );
});

/** Serve the bundle at ORIGIN; record every CSP violation the page sees. */
async function openBuilt(page: Page, url: string): Promise<string[]> {
  const csp = shippedCsp();
  await page.route(`${ORIGIN}/**`, (route) => {
    const { pathname } = new URL(route.request().url());
    const ext = path.extname(pathname);
    const file = ext
      ? path.join(dist, pathname)
      : path.join(dist, "index.html");
    const type = MIME[ext || ".html"] ?? "application/octet-stream";
    return route.fulfill({
      status: 200,
      contentType: type,
      headers: type === "text/html" ? { "Content-Security-Policy": csp } : {},
      body: readFileSync(file),
    });
  });
  const violations: string[] = [];
  await page.exposeFunction("__cspViolation", (v: string) =>
    violations.push(v)
  );
  await page.addInitScript(() => {
    document.addEventListener("securitypolicyviolation", (e) => {
      (
        window as unknown as { __cspViolation: (v: string) => void }
      ).__cspViolation(`${e.violatedDirective} ${e.blockedURI} ${e.sample}`);
    });
  });
  page.on("console", (m) => {
    if (/Content Security Policy/i.test(m.text())) violations.push(m.text());
  });
  await page.goto(`${ORIGIN}${url}`);
  return violations;
}

test("dashboard pages, dialog and select run under the shipped CSP", async ({
  page,
}) => {
  const violations = await openBuilt(page, "/dashboard/overview");
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
  for (const [route, name] of [
    ["cpu", "CPU"],
    ["memory", "Memory"],
    ["network", "Network"],
    ["timeline", "Timeline"],
  ]) {
    await page.goto(`${ORIGIN}/dashboard/${route}`);
    await expect(
      page.getByRole("heading", { name, exact: true }).first()
    ).toBeVisible();
  }
  // Radix dialog: react-remove-scroll injects a <style> while it is open.
  await page.goto(`${ORIGIN}/dashboard/power?scenario=unknown-chip`);
  await page.getByRole("button", { name: "Share sensor dump" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  // Settings: Radix select injects a <style> for its viewport.
  await page.goto(`${ORIGIN}/dashboard/settings`);
  await page.getByRole("combobox").first().click();
  await expect(page.getByRole("listbox")).toBeVisible();
  await page.keyboard.press("Escape");
  expect(violations).toEqual([]);
});

test("popover and onboarding windows run under the shipped CSP", async ({
  page,
}) => {
  const violations = await openBuilt(page, "/?window=popover");
  await expect(page.getByText("Open dashboard")).toBeVisible();
  await page.goto(`${ORIGIN}/?window=onboarding`);
  await expect(page.getByRole("button").first()).toBeVisible();
  expect(violations).toEqual([]);
});

test("the CSP is enforced: an inline script is blocked", async ({ page }) => {
  const violations = await openBuilt(page, "/dashboard/overview");
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
  // A script element added at runtime is what an injection would do.
  const ran = await page.evaluate(() => {
    const w = window as unknown as { __injected?: boolean };
    const s = document.createElement("script");
    s.textContent = "window.__injected = true";
    document.body.append(s);
    return w.__injected === true;
  });
  expect(ran).toBe(false);
  await expect.poll(() => violations.length).toBeGreaterThan(0);
});
