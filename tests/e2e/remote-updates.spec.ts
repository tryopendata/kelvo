import { expect, test } from "@playwright/test";

for (const theme of ["dark", "light"]) {
  test(`incompatible host release explains coordinated upgrade (${theme})`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 1280, height: 860 });
    await page.goto(
      `/?route=/dashboard/hosts&scenario=multi-host,remote-update-blocked&theme=${theme}&ticks=0`
    );
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    const guide = page.getByRole("button", { name: "Upgrade guide" });
    await guide.scrollIntoViewIfNeeded();
    await expect(page.getByText(/Update agent cannot apply it/)).toBeVisible();
    await expect(guide).toBeEnabled();
    await guide.click();
    await expect(page.getByRole("alert")).toHaveCount(0);
    await page.screenshot({
      path: `test-results/screens/remote-update-blocked-${theme}-${info.project.name}.png`,
    });
  });
}
