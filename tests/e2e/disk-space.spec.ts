import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

// The mock scan finishes 8 s after it starts (MOCK_SCAN_MS); the page's clock
// runs past it with the 1 Hz stream still ticking, so the page never goes stale.
const space = "/?route=/dashboard/disk%3Fview%3Dspace";

async function scanBoot(page: Page, theme = "dark") {
  await page.clock.install();
  await page.goto(`${space}&theme=${theme}`);
  await page.getByRole("button", { name: "Scan Macintosh HD" }).click();
  await expect(page.getByText(/Scanning · about/)).toBeVisible();
}

async function finish(page: Page) {
  await page.clock.runFor(9000);
  await expect(page.getByRole("button", { name: "Scan again" })).toBeVisible();
}

const tile = (page: Page, name: string) =>
  page.locator("[data-tile]").filter({ hasText: new RegExp(`^${name}`) });

test("the treemap fills in while scanning, then opens folders on click", async ({
  page,
}) => {
  await scanBoot(page);
  const map = page.getByRole("img", { name: /^Treemap of Macintosh HD/ });
  await expect(map.locator('[data-kind="unmeasured"]')).toHaveCount(1);
  await finish(page);
  await expect(map.locator('[data-kind="unmeasured"]')).toHaveCount(0);
  await expect(map.locator('[data-kind="other"]')).toHaveCount(1);
  await expect(map.locator('[data-kind="volume"]')).not.toHaveCount(0);

  // Hovering a row highlights its tile but leaves the tooltip to the map.
  await page
    .getByRole("table", { name: "Folder contents" })
    .getByRole("row", { name: /^Users, / })
    .hover();
  await expect(page.getByText("Folder · click to open")).toBeHidden();
  await tile(page, "Users").hover();
  await expect(page.getByText("Folder · click to open")).toBeVisible();
  await tile(page, "Users").click();
  const path = page.getByRole("navigation", { name: "Folder path" });
  await expect(path).toContainText("Users");
  await tile(page, "riley").click();
  await expect(
    page.getByRole("heading", { name: "In riley", exact: true })
  ).toBeVisible();
  // Every folder entry of riley has a tile or sits in smaller items.
  await expect(tile(page, "Library")).toBeVisible();
  await expect(page.locator('[data-kind="smaller"]')).toHaveCount(1);

  // Non-folder tiles do nothing on click.
  await path.getByRole("button", { name: "Macintosh HD" }).click();
  await page.locator('[data-kind="volume"]').first().click();
  await expect(
    page.getByRole("heading", { name: "In Macintosh HD" })
  ).toBeVisible();
});

for (const theme of ["dark", "light"]) {
  test(`the finished report passes axe (${theme})`, async ({ page }, info) => {
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await scanBoot(page, theme);
    await finish(page);
    await tile(page, "Users").click();
    await expect(
      page.getByRole("heading", { name: "In Users", exact: true })
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/disk-space-${theme}-${info.project.name}.png`,
      fullPage: true,
    });
    const result = await new AxeBuilder({ page }).include("main").analyze();
    expect(result.violations).toEqual([]);
  });
}

test("fits a narrow window without horizontal page scroll", async ({
  page,
}) => {
  await page.setViewportSize({ width: 760, height: 900 });
  await scanBoot(page);
  await finish(page);
  const overflow = await page
    .locator("main")
    .evaluate((m) => m.scrollWidth - m.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  const box = await page
    .getByRole("img", { name: /^Treemap of/ })
    .boundingBox();
  expect(box?.height).toBeGreaterThanOrEqual(220);
});
