import { expect, test } from "@playwright/test";

for (const theme of ["dark", "light"]) {
  test(`available update requires an explicit install (${theme})`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 1280, height: 860 });
    await page.goto(
      `/?window=dashboard&route=/dashboard/settings&theme=${theme}&ticks=0&update=available`
    );
    const install = page.getByRole("button", { name: "Install and restart" });
    await install.scrollIntoViewIfNeeded();
    await expect(install).toBeEnabled();
    await page.getByText("Release notes", { exact: true }).click();
    await page.screenshot({
      path: `test-results/screens/update-available-${theme}-${info.project.name}.png`,
    });
    await install.click();
    await expect(
      page.getByText("Update installed. Restarting Kelvo…")
    ).toBeVisible();
  });
  test(`manual update check with automatic checks off (${theme})`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 1280, height: 860 });
    await page.goto(
      `/?window=dashboard&route=/dashboard/settings&theme=${theme}&ticks=0`
    );
    const automatic = page.getByRole("switch", {
      name: "Check for updates automatically",
    });
    await automatic.scrollIntoViewIfNeeded();
    if (await automatic.isChecked()) await automatic.click();
    await expect(automatic).not.toBeChecked();
    await page.getByRole("button", { name: "Check now" }).click();
    await expect(
      page.getByText("You’re running the latest version.")
    ).toBeVisible();
    await page
      .getByRole("heading", { name: "General", exact: true })
      .scrollIntoViewIfNeeded();
    await page.screenshot({
      path: `test-results/screens/updates-${theme}-${info.project.name}.png`,
    });
  });
}
