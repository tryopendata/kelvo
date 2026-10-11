import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const overview = "/?route=/dashboard/containers&scenario=docker-32&ticks=0";
const detail =
  "/?route=/dashboard/containers/mock-orbstack/container-0&scenario=docker-32&ticks=0";

test("monitoring is opt-in and chooser preserves unavailable engines", async ({
  page,
}) => {
  await page.goto("/?route=/dashboard/containers&ticks=0");
  await expect(
    page.getByRole("button", { name: "Enable monitoring" })
  ).toBeVisible();
  await page.getByRole("button", { name: "Engine…" }).click();
  await expect(page.getByText("OrbStack · orbstack")).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Use this engine" }).last()
  ).toBeDisabled();
});

test("partial totals remain independent of search and back restores the view", async ({
  page,
}) => {
  await page.goto("/?route=/dashboard/containers&scenario=docker-200&ticks=0");
  await expect(page.getByText(/Partial population; totals/)).toBeVisible();
  await page.getByPlaceholder("Name, image or project").fill("worker");
  await expect(page.getByText("Now · 1 shown")).toBeVisible();
  await expect(page.getByText(/\/ 200 total/)).toBeVisible();
  await page.getByRole("link", { name: "worker", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "worker", exact: true })
  ).toBeVisible();
  await page.getByRole("button", { name: "← Containers", exact: true }).click();
  await expect(page.getByPlaceholder("Name, image or project")).toHaveValue(
    "worker"
  );
});

test("keyboard focus freezes ordering and supports explicit reorder and grouping", async ({
  page,
}) => {
  await page.goto(overview);
  const worker = page.getByRole("link", { name: "worker", exact: true });
  await worker.focus();
  await expect(page.getByText(/Order paused/)).toBeVisible();
  await page.getByRole("button", { name: "Reorder now" }).click();
  await expect(worker).toBeVisible();
  await page.getByRole("button", { name: /project-1 4 containers/ }).click();
  await expect(worker).toBeHidden();
  await page
    .getByRole("button", { name: /project-1 4 containers/ })
    .press("Enter");
  await expect(worker).toBeVisible();
});

test("removed detail preserves evidence and logs render only plain text", async ({
  page,
}) => {
  await page.goto(detail.replace("container-0", "removed"));
  await expect(
    page.getByText(/Removed container · retained history/)
  ).toBeVisible();
  await page.getByRole("button", { name: "Open logs", exact: true }).click();
  await expect(page.getByRole("region", { name: "Log output" })).toContainText(
    "<script>literal text</script>"
  );
  await expect(
    page.getByRole("region", { name: "Log output" }).locator("script")
  ).toHaveCount(0);
  await page.getByLabel("Follow", { exact: true }).check();
  await expect(page.getByText(/Following/)).toBeVisible();
  await page.getByRole("button", { name: "Close logs" }).click();
  await expect(page.getByRole("region", { name: "Log output" })).toBeHidden();
});

for (const theme of ["dark", "light"] as const) {
  test(`containers and diagnosis screenshots, ${theme}`, async ({
    page,
  }, info) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.goto(`${overview}&theme=${theme}`);
    await expect(
      page.getByRole("table", { name: "Containers now" })
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/containers-${theme}-${info.project.name}.png`,
    });
    const result = await new AxeBuilder({ page }).include("main").analyze();
    expect(result.violations).toEqual([]);
    await page.goto(`${detail}&theme=${theme}`);
    await expect(
      page.getByRole("heading", { name: "worker", exact: true })
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/container-detail-${theme}-${info.project.name}.png`,
      fullPage: true,
    });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.setViewportSize({ width: 720, height: 900 });
    await page.goto(`${overview}&theme=${theme}`);
    await expect(
      page.getByRole("table", { name: "Containers now" })
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/containers-narrow-reduced-${theme}-${info.project.name}.png`,
    });
    expect(
      await page
        .locator("main")
        .evaluate((main) => main.scrollWidth <= main.clientWidth)
    ).toBe(true);
  });
}

for (const count of [0, 1, 32, 128, 200]) {
  test(`${count} container population is explicit`, async ({ page }) => {
    await page.goto(
      `/?route=/dashboard/containers&scenario=docker-${count}&ticks=0`
    );
    await expect(page.getByText(`Now · ${count} shown`)).toBeVisible();
  });
}

test("historical workload keeps the inventory labelled Now", async ({
  page,
}) => {
  await page.goto(overview);
  await page.getByRole("button", { name: "Previous container range" }).click();
  await expect(
    page.getByRole("heading", {
      name: "Historical container workload · table below is Now",
    })
  ).toBeVisible();
  await expect(page.getByText("Now · 32 shown")).toBeVisible();
});

test("Follow keeps connected and scrolling up exposes the new-lines count", async ({
  page,
}) => {
  await page.goto(detail);
  await page.getByRole("button", { name: "Open logs", exact: true }).click();
  await page.getByLabel("Follow", { exact: true }).check();
  const output = page.getByRole("region", { name: "Log output" });
  await expect(output).toContainText("processed job 199");
  await output.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(
    page.getByRole("button", { name: /Autoscroll paused · [1-9]/ })
  ).toBeVisible();
  await page.getByRole("button", { name: /Jump to latest/ }).click();
  await expect(
    page.getByRole("button", { name: /Jump to latest/ })
  ).toBeHidden();
});

test("explicit monitored selection can be empty without reverting to automatic", async ({
  page,
}) => {
  await page.goto("/?route=/dashboard/containers&scenario=docker-1&ticks=0");
  await page
    .getByRole("checkbox", { name: "Monitor worker", exact: true })
    .uncheck();
  await expect(
    page.getByRole("checkbox", { name: "Monitor worker", exact: true })
  ).not.toBeChecked();
  await expect(page.getByText(/Partial population/)).toBeVisible();
});

test("local Docker surfaces stay scoped when switching to another Mac", async ({
  page,
}) => {
  await page.goto(
    "/?route=/dashboard/containers&scenario=docker-32,multi-host&ticks=0"
  );
  await expect(
    page.getByRole("table", { name: "Containers now" })
  ).toBeVisible();
  await page.getByRole("combobox", { name: "Selected machine" }).click();
  await page.getByRole("option", { name: "Studio Mac", exact: true }).click();
  await expect(
    page.getByText(/Container monitoring is available on This Mac/)
  ).toBeVisible();
  await expect(page.getByRole("table", { name: "Containers now" })).toHaveCount(
    0
  );
  await expect(
    page.getByRole("link", { name: "Containers", exact: true })
  ).toHaveCount(0);
  await page.getByRole("link", { name: "Timeline", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "Docker timeline" })
  ).toHaveCount(0);
  await page.getByRole("combobox", { name: "Selected machine" }).click();
  await page.getByRole("option", { name: "This Mac", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "Docker timeline" })
  ).toBeVisible();
  await page.getByRole("link", { name: "Containers", exact: true }).click();
  await page.getByRole("link", { name: "worker", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "worker", exact: true })
  ).toBeVisible();
  await page.getByRole("combobox", { name: "Selected machine" }).click();
  await page.getByRole("option", { name: "Studio Mac", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "worker", exact: true })
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Show containers on This Mac" })
    .click();
  await expect(
    page.getByRole("heading", { name: "worker", exact: true })
  ).toBeVisible();
});

test("retained investigation remains available after disabling and on a fresh disabled startup", async ({
  page,
}, info) => {
  await page.goto(overview);
  await page
    .getByRole("button", { name: "Disable monitoring", exact: true })
    .click();
  await expect(
    page.getByRole("combobox", { name: "Retained engine" })
  ).toBeVisible();
  await page.getByPlaceholder("Find retained containers").fill("Removed");
  await page.getByRole("link", { name: "Removed worker", exact: true }).click();
  await expect(page.getByText(/Last observed · CPU/)).toBeVisible();
  await page.getByRole("button", { name: "← Containers", exact: true }).click();
  await expect(page.getByPlaceholder("Find retained containers")).toHaveValue(
    "Removed"
  );
  await expect(
    page.getByRole("button", { name: "Enable monitoring" })
  ).toBeVisible();
  await page.goto(
    "/?route=/dashboard/containers&scenario=docker-retained&ticks=0"
  );
  await expect(
    page.getByRole("combobox", { name: "Retained engine" })
  ).toBeVisible();
  await page.getByRole("combobox", { name: "Retained engine" }).click();
  await page.getByRole("option", { name: "Colima · retained-colima" }).click();
  await expect(
    page.getByRole("link", { name: "Previous worker" })
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Enable monitoring" })
  ).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/docker-retained-disabled-${info.project.name}.png`,
    animations: "disabled",
  });
  await page.getByRole("link", { name: "Previous worker" }).click();
  await expect(page.getByText(/Retained engine · not selected/)).toBeVisible();
  await expect(page.getByText(/Last observed · CPU/)).toBeVisible();
});
