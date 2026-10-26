import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

const moduleHeader = (page: Page) =>
  page
    .locator("header")
    .filter({ has: page.getByRole("heading", { level: 1 }) });

test("fixed dates and Live work across Disk, Network and Power", async ({
  page,
}) => {
  await page.goto(
    "/?window=dashboard&route=/dashboard/disk&theme=dark&ticks=0"
  );
  await moduleHeader(page)
    .getByRole("button", { name: "Time range: 15m" })
    .click();
  await page.getByRole("radio", { name: "Date range", exact: true }).click();
  await page.getByLabel("From", { exact: true }).fill("2026-10-01 10:00");
  await page.getByLabel("To", { exact: true }).fill("2026-10-03 10:00");
  await page.getByRole("button", { name: "Apply", exact: true }).click();
  const live = page.getByRole("button", { name: "Live", exact: true });
  await expect(live).toHaveAttribute("aria-pressed", "false");
  await expect(page.getByText("Loading disk I/O…")).toHaveCount(0);
  const brush = page
    .getByRole("slider", { name: "Select a time range" })
    .first();
  await brush.focus();
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("selection-summary")).toContainText("Oct 3");
  await live.click();
  await expect(live).toHaveAttribute("aria-pressed", "true");
  await expect(
    moduleHeader(page).getByRole("button", { name: "Time range: 2d" })
  ).toBeVisible();
  await moduleHeader(page)
    .getByRole("button", { name: "Time range: 2d" })
    .click();
  await page.getByRole("radio", { name: "Date range", exact: true }).click();
  await page.getByLabel("From", { exact: true }).fill("2026-10-01 10:00");
  await page.getByLabel("To", { exact: true }).fill("2026-10-03 10:00");
  await page.getByRole("button", { name: "Apply", exact: true }).click();
  await page.locator("[data-sidebar-item][href='/dashboard/network']").click();
  await expect(
    page.getByRole("img", { name: /Upload above the line.*selected dates/ })
  ).toBeVisible();
  await expect(page.getByText("Loading chart history…")).toHaveCount(0);
  await page.locator("[data-sidebar-item][href='/dashboard/power']").click();
  await expect(
    page.getByRole("heading", { name: "Power draw, selected dates" })
  ).toBeVisible();
  await expect(
    page.getByText("App power lines are available for ranges up to 1 hour.")
  ).toBeVisible();
  await moduleHeader(page)
    .getByRole("radio", { name: "5m", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "Power draw, last 5 minutes" })
  ).toBeVisible();
  await expect(
    page
      .getByRole("region", { name: "Battery charge" })
      .getByRole("radio", { name: "5m", exact: true })
  ).toBeChecked();
});

/**
 * One chart window for every module page (D-091): a choice made on CPU is
 * the one Memory and Power open with. In-app navigation only; the mock keeps
 * settings in memory, so a reload would reset them.
 */
test("a window picked on CPU carries to Memory and Power", async ({ page }) => {
  await page.goto("/?window=dashboard&route=/dashboard/cpu&theme=dark");
  await moduleHeader(page)
    .getByRole("button", { name: "Time range: 15m" })
    .click();
  await page.getByRole("button", { name: "Last 30 minutes" }).click();
  await expect(
    moduleHeader(page).getByRole("button", { name: "Time range: 30m" })
  ).toBeVisible();

  await page.locator("[data-sidebar-item][href='/dashboard/memory']").click();
  await expect(
    page.getByRole("heading", { name: "Memory", exact: true })
  ).toBeVisible();
  await expect(
    moduleHeader(page).getByRole("button", { name: "Time range: 30m" })
  ).toBeVisible();
  await expect(page.getByText("Swap, last 30 minutes")).toBeVisible();

  await page.locator("[data-sidebar-item][href='/dashboard/power']").click();
  await expect(
    page.getByRole("heading", { name: "Power & Sensors", exact: true })
  ).toBeVisible();
  await expect(
    moduleHeader(page).getByRole("button", { name: "Time range: 30m" })
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Power draw, last 30 minutes" })
  ).toBeVisible();
  await expect(
    page
      .getByRole("region", { name: "Battery charge" })
      .getByRole("button", { name: "Time range: 30m" })
  ).toBeVisible();
});

for (const theme of ["dark", "light"] as const) {
  test(`long and fixed ranges in ${theme}`, async ({ page }) => {
    await page.goto(`/?window=dashboard&route=/dashboard/cpu&theme=${theme}`);
    await moduleHeader(page)
      .getByRole("button", { name: "Time range: 15m" })
      .click();
    await expect(
      page.getByRole("button", { name: "Last 90 days" })
    ).toBeVisible();
    const picker = await new AxeBuilder({ page })
      .include('[aria-label="Choose time range"]')
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(picker.violations).toEqual([]);
    await page.getByRole("button", { name: "Last 90 days" }).click();
    const trigger = await new AxeBuilder({ page })
      .include('button[aria-label="Time range: 90d"]')
      .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
      .analyze();
    expect(trigger.violations).toEqual([]);
    await expect(
      page.getByRole("heading", { name: "Per-core load, last 90 days" })
    ).toBeVisible();
    // The heatmap stays bounded even at the longest supported range.
    const heatmap = page.getByRole("img", {
      name: /^Per-core load, last 90 days/,
    });
    await expect(heatmap.locator("[data-gap]")).not.toHaveCount(0);
    const cells = await heatmap.locator(".h-2\\.5").count();
    expect(cells).toBeGreaterThan(0);
    expect(cells).toBeLessThanOrEqual(14 * 61);
    await moduleHeader(page)
      .getByRole("button", { name: "Time range: 90d" })
      .click();
    await page.getByRole("button", { name: "Last 7 days" }).click();
    await expect(
      page.getByRole("heading", { name: "Per-core load, last 7 days" })
    ).toBeVisible();
    await expect(page.getByText("Loading chart history…")).toHaveCount(0);
    await moduleHeader(page)
      .getByRole("button", { name: "Time range: 7d" })
      .click();
    await page.getByRole("radio", { name: "Date range", exact: true }).click();
    await page.getByLabel("From", { exact: true }).fill("2026-10-01 10:00");
    await page.getByLabel("To", { exact: true }).fill("2026-10-01 16:00");
    await page.getByRole("button", { name: "Apply", exact: true }).click();
    await expect(
      moduleHeader(page).getByRole("button", { name: "Time range: Custom" })
    ).toBeVisible();
    await expect(
      page.getByRole("heading", { name: "Per-core load, selected dates" })
    ).toBeVisible();
    await expect(
      page.getByRole("status").filter({
        hasText: /2026-10-01 10:00 to 2026-10-01 16:00/,
      })
    ).toBeVisible();
    await moduleHeader(page)
      .getByRole("radio", { name: "5m", exact: true })
      .click();
    await expect(
      moduleHeader(page).getByRole("radio", { name: "5m", exact: true })
    ).toBeChecked();
    await expect(
      page.getByRole("heading", { name: "Per-core load, last 5 minutes" })
    ).toBeVisible();
  });
}
