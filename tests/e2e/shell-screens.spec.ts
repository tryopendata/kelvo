import { expect, type Page, test } from "@playwright/test";

/**
 * Popover, dashboard shell, Overview and the plan 4.17 states on the mock
 * transport, in both themes. Screenshots land in test-results/screens/ for
 * the visual review (verification.md); they are not
 * compared against a baseline.
 */
const THEMES = ["light", "dark"] as const;

function trackConsoleErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  page.on("pageerror", (err) => errors.push(err.message));
  return errors;
}

const shot = (page: Page, name: string, project: string, fullPage = false) =>
  page.screenshot({
    path: `test-results/screens/${name}-${project}.png`,
    fullPage,
  });

const popoverViewport = (page: Page) =>
  page.locator("[data-radix-scroll-area-viewport]");

for (const theme of THEMES) {
  test(`popover (${theme}) default and scrolled`, async ({ page }, info) => {
    const errors = trackConsoleErrors(page);
    await page.setViewportSize({ width: 360, height: 760 });
    await page.goto(`/?window=popover&theme=${theme}&ticks=0`);
    await expect(page.getByText("Open dashboard")).toBeVisible();
    await expect(page.getByText("Battery", { exact: true })).toBeAttached();
    await shot(page, `popover-${theme}`, info.project.name);
    await popoverViewport(page).evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    await expect(page.getByText("Battery", { exact: true })).toBeInViewport();
    await shot(page, `popover-${theme}-scrolled`, info.project.name);
    expect(errors).toEqual([]);
  });

  test(`overview (${theme})`, async ({ page }, info) => {
    const errors = trackConsoleErrors(page);
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(`/?window=dashboard&theme=${theme}&ticks=0`);
    await expect(
      page.getByRole("heading", { name: "Overview", exact: true })
    ).toBeVisible();
    // Process lists fill from the interest the page registers.
    await expect(
      page.getByRole("list", { name: "Top processes by CPU" })
    ).toContainText("Xcode");
    await shot(page, `overview-${theme}`, info.project.name);
    expect(errors).toEqual([]);
  });
}

test("overview cards open their module page", async ({ page }) => {
  await page.goto("/?window=dashboard&ticks=0");
  await page
    .getByRole("link", { name: /^Memory/ })
    .last()
    .click();
  await expect(
    page.getByRole("heading", { name: "Memory", exact: true })
  ).toBeVisible();
});

test("a sidebar tab opens its page scrolled to the top", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 600 });
  await page.goto("/?window=dashboard&ticks=0");
  await expect(
    page.getByRole("list", { name: "Top processes by CPU" })
  ).toContainText("Xcode");
  const main = page.locator("main");
  await main.evaluate((el) => {
    el.scrollTop = el.scrollHeight;
  });
  expect(await main.evaluate((el) => el.scrollTop)).toBeGreaterThan(0);
  await page
    .getByRole("navigation")
    .getByRole("link", { name: /^Network/ })
    .click();
  await expect(
    page.getByRole("heading", { name: "Network", exact: true })
  ).toBeVisible();
  expect(await main.evaluate((el) => el.scrollTop)).toBe(0);
});

test("unknown chip hides Power & Sensors and offers the sensor dump", async ({
  page,
}, info) => {
  const errors = trackConsoleErrors(page);
  await page.setViewportSize({ width: 1280, height: 1000 });
  await page.goto(
    "/?window=dashboard&theme=dark&ticks=0&scenario=unknown-chip"
  );
  await expect(page.getByText(/aren.t mapped for this chip/)).toBeVisible();
  await expect(
    page.getByRole("navigation").getByRole("link", { name: /Power/ })
  ).toHaveCount(0);
  await page.getByRole("button", { name: "Share sensor dump" }).click();
  await expect(page.getByTestId("sensor-dump-json")).toContainText("model");
  await shot(page, "state-unknown-chip-dump", info.project.name);
  await page.keyboard.press("Escape");
  await shot(page, "state-unknown-chip", info.project.name, true);

  await page.setViewportSize({ width: 360, height: 760 });
  await page.goto("/?window=popover&theme=dark&ticks=0&scenario=unknown-chip");
  await expect(page.getByText("Open dashboard")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Network", exact: true })
  ).toBeAttached();
  await expect(
    page.getByRole("heading", { name: "Power", exact: true })
  ).toHaveCount(0);
  await shot(page, "state-unknown-chip-popover", info.project.name);
  expect(errors).toEqual([]);
});

test("no battery and no fans", async ({ page }, info) => {
  await page.setViewportSize({ width: 1280, height: 833 });
  await page.goto("/?window=dashboard&theme=dark&ticks=0&scenario=no-battery");
  await expect(page.getByText("Mac mini (M4 Pro)")).toBeVisible();
  await expect(
    page.getByRole("navigation").getByRole("link", { name: /Battery/ })
  ).toHaveCount(0);
  await shot(page, "state-no-battery", info.project.name);

  await page.goto("/?window=dashboard&theme=dark&ticks=0&scenario=no-fans");
  await expect(page.getByText("Passive cooling")).toBeVisible();
  await shot(page, "state-no-fans", info.project.name);
});

test("paused and stale", async ({ page }, info) => {
  await page.setViewportSize({ width: 1280, height: 833 });
  await page.goto("/?window=dashboard&theme=dark&ticks=0&scenario=paused");
  await expect(page.getByRole("main").getByText("Paused")).toBeVisible();
  await shot(page, "state-paused", info.project.name);

  // Stale: no frame for three intervals.
  await page.goto("/?window=dashboard&theme=dark&ticks=0&scenario=stale");
  await expect(page.locator("main[data-stale]")).toBeVisible({
    timeout: 6000,
  });
  await expect(page.getByText("Stale · no new data")).toBeVisible();
  await shot(page, "state-stale", info.project.name);

  await page.setViewportSize({ width: 360, height: 760 });
  await page.goto("/?window=popover&theme=dark&ticks=0&scenario=stale");
  await expect(page.getByText("Stale", { exact: true })).toBeVisible({
    timeout: 6000,
  });
  await shot(page, "state-stale-popover", info.project.name);
});
