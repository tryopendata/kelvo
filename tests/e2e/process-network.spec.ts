import { expect, type Page, test } from "@playwright/test";

/**
 * Per-process network (v1.2 phase 1.2-A, D-081) on the mock transport: the
 * Overview Network card's top 5, the Network page's Apps table (D-089) and the
 * Processes page's Network column set, in both themes and with the
 * capability absent. Screenshots land in test-results/screens/ for the
 * visual review (verification.md); they are not compared against a
 * baseline.
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

const shot = (page: Page, name: string, project: string) =>
  page.screenshot({ path: `test-results/screens/${name}-${project}.png` });

const networkCard = (page: Page) =>
  page.getByRole("link", { name: /^Network/ }).last();

for (const theme of THEMES) {
  test(`overview network card lists your processes (${theme})`, async ({
    page,
  }, info) => {
    const errors = trackConsoleErrors(page);
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(`/?window=dashboard&theme=${theme}&ticks=0`);
    const list = page.getByRole("list", {
      name: "Top processes by network rate",
    });
    await expect(list.getByRole("listitem")).toHaveCount(5);
    await expect(list.getByRole("listitem").first()).toContainText(
      "Safari22.1 MB/s"
    );
    await expect(networkCard(page)).toContainText("Your processes only");
    await networkCard(page).screenshot({
      path: `test-results/screens/overview-network-card-${theme}-${info.project.name}.png`,
    });
    await shot(page, `overview-process-network-${theme}`, info.project.name);
    expect(errors).toEqual([]);
  });

  test(`network page apps table (${theme})`, async ({ page }, info) => {
    const errors = trackConsoleErrors(page);
    await page.setViewportSize({ width: 1280, height: 1100 });
    await page.goto(
      `/?window=dashboard&route=/dashboard/network&theme=${theme}&ticks=0`
    );
    const table = page.getByRole("table", { name: "Network by app" });
    await expect(
      page.getByRole("heading", { name: "Apps, last 15 minutes" })
    ).toBeVisible();
    await expect(
      table.getByRole("row", { name: /System and other/ })
    ).toBeVisible();
    await expect(
      table.getByRole("columnheader", { name: /Total/ })
    ).toHaveAttribute("aria-sort", "descending");
    await page.screenshot({
      path: `test-results/screens/network-apps-${theme}-${info.project.name}.png`,
      fullPage: true,
    });
    expect(errors).toEqual([]);
  });

  test(`processes page network columns (${theme})`, async ({ page }, info) => {
    const errors = trackConsoleErrors(page);
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(
      `/?window=dashboard&route=/dashboard/processes&theme=${theme}&ticks=0`
    );
    await page.getByRole("radio", { name: "Network" }).click();
    const table = page.getByRole("table", { name: "Processes" });
    await expect(
      table.getByRole("columnheader", { name: /Net total/ })
    ).toBeVisible();
    // Asking for rates brings a batch that carries them (no tick needed).
    await expect(table.getByRole("row").nth(1)).toContainText("Safari");
    await expect(table.getByRole("row").nth(1)).toContainText("22.1 MB/s");
    await shot(page, `processes-network-${theme}`, info.project.name);
    expect(errors).toEqual([]);
  });
}

test("without per-process network the interface list and columns stay as before", async ({
  page,
}, info) => {
  const errors = trackConsoleErrors(page);
  await page.setViewportSize({ width: 1280, height: 1100 });
  await page.goto(
    "/?window=dashboard&theme=dark&ticks=0&scenario=no-process-network"
  );
  await expect(
    page.getByRole("list", { name: "Interfaces by total rate" })
  ).toBeVisible();
  await expect(networkCard(page).getByText("Your processes only")).toHaveCount(
    0
  );
  await networkCard(page).screenshot({
    path: `test-results/screens/overview-network-card-absent-${info.project.name}.png`,
  });

  await page.getByRole("link", { name: "Network", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Interfaces", exact: true })
  ).toBeVisible();
  await expect(page.getByRole("heading", { name: /^Apps/ })).toHaveCount(0);
  await expect(page.getByRole("slider")).toHaveCount(0);

  await page.getByRole("link", { name: "Processes", exact: true }).click();
  await expect(page.getByRole("radio", { name: "Disk" })).toBeVisible();
  await expect(page.getByRole("radio", { name: "Network" })).toHaveCount(0);
  expect(errors).toEqual([]);
});
