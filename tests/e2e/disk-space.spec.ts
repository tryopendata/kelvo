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

const tile = (page: Page, name: string, depth = 1) =>
  page
    .locator(`[data-tile][data-depth="${depth}"]`)
    .filter({ hasText: new RegExp(`^${name}`) });
/** A frame opens from its name row; its own entries fill the rest of it. */
const header = { position: { x: 24, y: 8 } };

test("the treemap fills in while scanning, then opens folders two levels deep", async ({
  page,
}) => {
  await scanBoot(page);
  const map = page.getByRole("img", { name: /^Treemap of Macintosh HD/ });
  await expect(map.locator('[data-kind="unmeasured"]')).toHaveCount(1);
  // The running home folder is drawn as it grows, with what it holds so far.
  await expect(map.locator("[data-live]").first()).toContainText("so far");
  await finish(page);
  await expect(map.locator('[data-kind="unmeasured"]')).toHaveCount(0);
  await expect(map.locator("[data-live]")).toHaveCount(0);
  await expect(map.locator('[data-kind="other"]')).toHaveCount(1);
  await expect(map.locator('[data-kind="volume"]')).not.toHaveCount(0);

  // Users holds one home folder, so its frame shows that folder's entries.
  await expect(tile(page, "Users › riley")).toBeVisible();
  // Hovering a row highlights its tile but leaves the tooltip to the map.
  await page
    .getByRole("table", { name: "Folder contents" })
    .getByRole("row", { name: /^Users, / })
    .hover();
  await expect(page.getByText("Folder · click to open")).toBeHidden();
  await tile(page, "Library", 2).first().hover();
  await expect(page.getByText(/% of Users ·/)).toBeVisible();
  await expect(page.getByText("Folder · click to open")).toBeVisible();

  // A nested folder opens directly, two levels down.
  await tile(page, "Library", 2).first().click();
  const path = page.getByRole("navigation", { name: "Folder path" });
  await expect(path).toContainText(/Usersriley.*Library$/);
  await path.getByRole("button", { name: "Macintosh HD" }).click();
  // The frame's name row opens the folder its entries belong to.
  await tile(page, "Users › riley").click(header);
  await expect(
    page.getByRole("heading", { name: "In riley", exact: true })
  ).toBeVisible();
  await expect(tile(page, "Library")).toBeVisible();

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
    await tile(page, "Applications").click(header);
    await expect(
      page.getByRole("heading", { name: "In Applications", exact: true })
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
