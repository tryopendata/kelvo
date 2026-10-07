import { expect, type Page, test } from "@playwright/test";

/**
 * Network, Disk, Battery and Processes pages on the mock transport, in both
 * themes. Screenshots land in test-results/screens/ for the visual
 * review (verification.md); they are not compared against a baseline.
 */
const THEMES = ["light", "dark"] as const;

const PAGES = [
  { route: "/dashboard/network", heading: "Network", slug: "network" },
  { route: "/dashboard/disk", heading: "Disk", slug: "disk" },
  { route: "/dashboard/battery", heading: "Battery", slug: "battery" },
  { route: "/dashboard/processes", heading: "Processes", slug: "processes" },
] as const;

function trackConsoleErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  page.on("pageerror", (err) => errors.push(err.message));
  return errors;
}

for (const theme of THEMES) {
  for (const p of PAGES) {
    test(`${p.slug} page (${theme}) renders without console errors`, async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await page.setViewportSize({ width: 1280, height: 1000 });
      await page.goto(
        `/?window=dashboard&route=${p.route}&theme=${theme}&ticks=0`
      );
      await expect(
        page.getByRole("heading", { name: p.heading, exact: true })
      ).toBeVisible();
      await page.screenshot({
        path: `test-results/screens/${p.slug}-${theme}-${info.project.name}.png`,
        fullPage: true,
      });
      expect(errors).toEqual([]);
    });
  }
}

for (const p of [
  {
    route: "/dashboard/network",
    chart: /^Upload above the line/,
    row: "Upload",
  },
  { route: "/dashboard/disk", chart: /^Read above the line/, row: "Read" },
]) {
  test(`hovering a ${p.route} bar shows its values`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.goto(`/?window=dashboard&route=${p.route}&ticks=0`);
    const chart = page.getByRole("img", { name: p.chart });
    const box = await chart.boundingBox();
    if (!box) throw new Error("chart not laid out");
    await page.mouse.move(box.x + box.width - 4, box.y + box.height / 2);
    const tip = page.getByRole("tooltip");
    await expect(tip).toBeVisible();
    await expect(tip).toContainText(p.row);
    await page.mouse.move(box.x + box.width / 2, box.y + box.height + 200);
    await expect(tip).toHaveCount(0);
  });
}

test("quitting a process asks first, then removes it", async ({ page }) => {
  await page.goto("/?window=dashboard&route=/dashboard/processes&ticks=0");
  const row = page.getByRole("row", { name: /Xcode/ });
  await row.hover();
  await row.getByRole("button", { name: /^Quit Xcode/ }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toContainText("Quit Xcode?");
  await dialog.getByRole("button", { name: "Quit", exact: true }).click();
  await expect(page.getByText(/Asked Xcode \(\d+\) to quit/)).toBeVisible();
  await expect(page.getByRole("row", { name: /Xcode/ })).toHaveCount(0);
});

test("battery page on a desktop goes back to Overview", async ({ page }) => {
  await page.goto(
    "/?window=dashboard&route=/dashboard/battery&ticks=0&scenario=no-battery"
  );
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
});
