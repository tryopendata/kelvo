import { expect, test } from "@playwright/test";

/**
 * One chart window for every module page (D-091): a choice made on CPU is
 * the one Memory and Power open with. In-app navigation only; the mock keeps
 * settings in memory, so a reload would reset them.
 */
test("a window picked on CPU carries to Memory and Power", async ({ page }) => {
  await page.goto("/?window=dashboard&route=/dashboard/cpu&theme=dark");
  const thirty = page.getByRole("radio", { name: "30m", exact: true });
  await thirty.click();
  await expect(thirty).toBeChecked();

  await page.locator("[data-sidebar-item][href='/dashboard/memory']").click();
  await expect(
    page.getByRole("heading", { name: "Memory", exact: true })
  ).toBeVisible();
  await expect(
    page.getByRole("radio", { name: "30m", exact: true })
  ).toBeChecked();
  await expect(page.getByText("Swap, last 30 minutes")).toBeVisible();

  await page.locator("[data-sidebar-item][href='/dashboard/power']").click();
  await expect(
    page.getByRole("heading", { name: "Power & Sensors", exact: true })
  ).toBeVisible();
  await expect(
    page.getByRole("radio", { name: "30m", exact: true })
  ).toBeChecked();
  await expect(
    page.getByText("Power by component, last 30 minutes")
  ).toBeVisible();
});
