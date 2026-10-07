import { expect, type Page, test } from "@playwright/test";

/**
 * Per-process GPU time (v1.2 phase 1.2-B, D-085) on the mock transport: the
 * Overview GPU card's top 5, the GPU page's process table and the Processes
 * page's GPU column set, in both themes and with the capability absent.
 * Screenshots land in test-results/screens/ for the visual review
 * (verification.md); they are not compared against a baseline.
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

const gpuCard = (page: Page) => page.getByRole("link", { name: /^GPU/ }).last();

for (const theme of THEMES) {
  test(`overview GPU card lists your processes (${theme})`, async ({
    page,
  }, info) => {
    const errors = trackConsoleErrors(page);
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(`/?window=dashboard&theme=${theme}&ticks=0`);
    const list = page.getByRole("list", { name: "Top processes by GPU" });
    await expect(list.getByRole("listitem")).toHaveCount(5);
    await expect(list.getByRole("listitem").first()).toContainText(
      "WindowServer14.2%"
    );
    await expect(gpuCard(page)).toContainText("Your processes only");
    await expect(
      gpuCard(page).getByRole("img", { name: /GPU, last/ })
    ).toHaveCount(0);
    await gpuCard(page).screenshot({
      path: `test-results/screens/overview-gpu-card-${theme}-${info.project.name}.png`,
    });
    await page.screenshot({
      path: `test-results/screens/overview-process-gpu-${theme}-${info.project.name}.png`,
    });
    expect(errors).toEqual([]);
  });

  test(`GPU page apps table (${theme})`, async ({ page }, info) => {
    const errors = trackConsoleErrors(page);
    await page.setViewportSize({ width: 1280, height: 1100 });
    await page.goto(
      `/?window=dashboard&route=/dashboard/gpu&theme=${theme}&ticks=0`
    );
    const table = page.getByRole("table", { name: "GPU by app" });
    // WindowServer is another user's: it is in System and other (D-099).
    await expect(
      table.getByText("System and other", { exact: true })
    ).toBeVisible();
    await expect(
      table.getByRole("columnheader", { name: /Avg GPU/ })
    ).toHaveAttribute("aria-sort", "descending");
    await expect(page.getByText(/long GPU compute job/)).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/gpu-processes-${theme}-${info.project.name}.png`,
      fullPage: true,
    });
    expect(errors).toEqual([]);
  });

  test(`processes page GPU columns (${theme})`, async ({ page }, info) => {
    const errors = trackConsoleErrors(page);
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(
      `/?window=dashboard&route=/dashboard/processes&theme=${theme}&ticks=0`
    );
    await page.getByRole("radio", { name: "GPU" }).click();
    const table = page.getByRole("table", { name: "Processes" });
    await expect(
      table.getByRole("columnheader", { name: /% GPU/ })
    ).toBeVisible();
    // Asking for GPU time brings a batch that carries it (no tick needed).
    await expect(table.getByRole("row").nth(1)).toContainText("WindowServer");
    await expect(table.getByRole("row").nth(1)).toContainText("14.2");
    await page.screenshot({
      path: `test-results/screens/processes-gpu-${theme}-${info.project.name}.png`,
    });
    expect(errors).toEqual([]);
  });
}

test("without per-process GPU the chart stays and no GPU columns show", async ({
  page,
}, info) => {
  const errors = trackConsoleErrors(page);
  await page.setViewportSize({ width: 1280, height: 1100 });
  await page.goto(
    "/?window=dashboard&theme=dark&ticks=0&scenario=no-process-gpu"
  );
  await expect(
    gpuCard(page).getByRole("img", { name: /GPU, last/ })
  ).toBeVisible();
  await expect(
    page.getByRole("list", { name: "Top processes by GPU" })
  ).toHaveCount(0);
  await gpuCard(page).screenshot({
    path: `test-results/screens/overview-gpu-card-absent-${info.project.name}.png`,
  });

  await page.getByRole("link", { name: "GPU", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "GPU", exact: true })
  ).toBeVisible();
  await expect(page.getByRole("table", { name: "GPU by app" })).toHaveCount(0);

  await page.getByRole("link", { name: "Processes", exact: true }).click();
  await expect(page.getByRole("radio", { name: "Network" })).toBeVisible();
  await expect(page.getByRole("radio", { name: "GPU" })).toHaveCount(0);
  expect(errors).toEqual([]);
});
