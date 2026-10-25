import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const overview = "/?route=/dashboard/containers&scenario=docker-32&ticks=0";

test("volume cleanup requires a separate confirmation and refreshes the inventory", async ({
  page,
}) => {
  await page.goto(overview);
  await page.getByRole("radio", { name: "Storage & cleanup" }).click();
  await expect(
    page.getByRole("button", { name: "Review cleanup" })
  ).toBeDisabled();
  await page
    .getByRole("checkbox", { name: "Select old-database-data" })
    .check();
  await page.getByRole("button", { name: "Review cleanup" }).click();
  await expect(page.getByRole("dialog")).toContainText("old-database-data");
  await page.getByRole("button", { name: "Review volume deletion" }).click();
  await expect(
    page.getByRole("button", { name: "Delete selected resources" })
  ).toBeDisabled();
  await expect(page.getByRole("dialog")).toContainText(
    "Docker volume names can be reused"
  );
  await page.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(
    page.getByRole("checkbox", { name: "Select old-database-data" })
  ).toBeChecked();
  await page.getByRole("button", { name: "Review cleanup" }).click();
  await page.getByRole("button", { name: "Review volume deletion" }).click();
  await page
    .getByRole("checkbox", { name: /I understand the permanent/ })
    .check();
  await page.getByRole("button", { name: "Delete selected resources" }).click();
  await expect(
    page.getByRole("heading", { name: "Operation complete" })
  ).toBeVisible();
  await expect(
    page.getByRole("region", { name: "Docker operation results" })
  ).toContainText("Succeeded");
  await expect(
    page.getByRole("checkbox", { name: "Select old-database-data" })
  ).toHaveCount(0);
});

test("Activity row Stop keeps AutoRemove disabled and confirms ordinary containers", async ({
  page,
}) => {
  await page.goto(overview);
  const table = page.getByRole("table", { name: "Containers now" });
  await expect(
    table.getByRole("button", { name: "Stop web", exact: true })
  ).toHaveAttribute("aria-disabled", "true");
  await table.getByRole("button", { name: "Stop worker", exact: true }).click();
  await expect(page.getByRole("alertdialog")).toContainText("30 seconds");
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "Stop", exact: true })
    .click();
  await expect(
    table.getByRole("button", { name: "Start worker", exact: true })
  ).toBeAttached();
});

for (const theme of ["dark", "light"] as const) {
  test(`storage filters, accessibility and screenshot, ${theme}`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(
      `/?route=/dashboard/containers&scenario=docker-32&theme=${theme}`
    );
    await page.getByRole("radio", { name: "Storage & cleanup" }).click();
    await expect(
      page.getByRole("table", { name: "Storage resources" })
    ).toBeVisible();
    await page
      .getByRole("searchbox", { name: "Search Docker storage resources" })
      .fill("database");
    await expect(page.getByText("Previous use unknown")).toBeVisible();
    await page.getByRole("button", { name: "Clear search" }).click();
    await page.screenshot({
      path: `test-results/screens/docker-storage-${theme}-${info.project.name}.png`,
    });
    const result = await new AxeBuilder({ page }).include("main").analyze();
    expect(result.violations).toEqual([]);
  });
}
