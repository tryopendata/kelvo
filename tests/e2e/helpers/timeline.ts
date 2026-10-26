import { expect, type Page } from "@playwright/test";

export async function chooseTimelinePreset(page: Page, label: string) {
  await page.getByRole("button", { name: /^Time range:/ }).click();
  await page.getByRole("button", { name: label, exact: true }).click();
  await expect(
    page.getByRole("group", { name: "Preset time ranges" })
  ).toHaveCount(0);
}
