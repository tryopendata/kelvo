import { expect, type Page, test } from "@playwright/test";

/**
 * CPU, GPU, Memory and Power & Sensors pages on the mock transport, in both
 * themes. Screenshots land in test-results/screens/ for the visual
 * review (verification.md); they are not compared against a baseline.
 */
const THEMES = ["light", "dark"] as const;

const PAGES = [
  { route: "/dashboard/cpu", heading: "CPU", slug: "cpu" },
  { route: "/dashboard/gpu", heading: "GPU", slug: "gpu" },
  { route: "/dashboard/memory", heading: "Memory", slug: "memory" },
  { route: "/dashboard/power", heading: "Power & Sensors", slug: "power" },
] as const;

function trackConsoleErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  page.on("pageerror", (err) => errors.push(err.message));
  return errors;
}

for (const theme of THEMES) {
  for (const p of PAGES) {
    test(`${p.slug} page (${theme}) renders without console errors`, async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await page.setViewportSize({ width: 1280, height: 1000 });
      await page.goto(
        `/?window=dashboard&route=${p.route}&theme=${theme}&ticks=0`
      );
      await expect(
        page.getByRole("heading", { name: p.heading, exact: true })
      ).toBeVisible();
      await page.screenshot({
        path: `test-results/screens/${p.slug}-${theme}-${info.project.name}.png`,
        fullPage: true,
      });
      expect(errors).toEqual([]);
    });
  }
}

test("cpu apps table fills in and Every process opens Processes", async ({
  page,
}) => {
  await page.goto("/?window=dashboard&route=/dashboard/cpu&ticks=0");
  await expect(page.getByRole("table", { name: "CPU by app" })).toBeVisible();
  await page.getByRole("button", { name: "Every process" }).click();
  await expect(
    page.getByRole("heading", { name: "Processes", exact: true })
  ).toBeVisible();
});

// Dragging across the lead chart scopes the page's apps table (D-099).
for (const p of [
  { route: "/dashboard/cpu", noun: "CPU" },
  { route: "/dashboard/gpu", noun: "GPU" },
  { route: "/dashboard/memory", noun: "Memory" },
  { route: "/dashboard/disk", noun: "Disk" },
] as const) {
  test(`dragging on the ${p.noun} chart scopes its apps table`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.goto(`/?window=dashboard&route=${p.route}&ticks=0`);
    await expect(
      page.getByRole("heading", { name: new RegExp(`^${p.noun} by app, last`) })
    ).toBeVisible();
    const brush = page.getByRole("slider", { name: "Select a time range" });
    const box = await brush.first().boundingBox();
    if (!box) throw new Error("chart not laid out");
    await page.mouse.move(box.x + box.width * 0.5, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.7, box.y + box.height / 2, {
      steps: 5,
    });
    await page.mouse.up();
    await expect(
      page.getByRole("heading", {
        name: new RegExp(`^${p.noun} by app, selected`),
      })
    ).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(
      page.getByRole("heading", { name: new RegExp(`^${p.noun} by app, last`) })
    ).toBeVisible();
  });
}

test("power page without fans reads passive cooling", async ({ page }) => {
  await page.goto(
    "/?window=dashboard&route=/dashboard/power&ticks=0&scenario=no-fans"
  );
  await expect(page.getByText("Passive cooling")).toBeVisible();
});

test("power page on an unknown chip offers the sensor dump", async ({
  page,
}) => {
  await page.goto(
    "/?window=dashboard&route=/dashboard/power&ticks=0&scenario=unknown-chip"
  );
  await page.getByRole("button", { name: "Share sensor dump" }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
});

test("cpu page across a sleep gap renders without console errors", async ({
  page,
}, info) => {
  const errors: string[] = [];
  page.on("pageerror", (err) => errors.push(err.message));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  await page.goto(
    "/?window=dashboard&route=/dashboard/cpu&theme=dark&ticks=0&scenario=sleep-gap"
  );
  await expect(
    page.getByRole("heading", { name: "CPU", exact: true })
  ).toBeVisible();
  // The mock's sleep inside the last hour, labelled on the 1h chart.
  await page.getByRole("radio", { name: "1h", exact: true }).click();
  const asleep = page.getByRole("note", {
    name: /^Asleep .* not interpolated$/,
  });
  await expect(asleep.first()).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/cpu-sleep-gap-${info.project.name}.png`,
    fullPage: true,
  });
  expect(errors).toEqual([]);
});

test("module page charts label a sleep gap", async ({ page }, info) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  for (const route of ["gpu", "memory", "network", "disk"]) {
    await page.goto(
      `/?window=dashboard&route=/dashboard/${route}&theme=dark&ticks=0&scenario=sleep-gap`
    );
    await page.getByRole("radio", { name: "1h", exact: true }).click();
    await expect(
      page.getByRole("note", { name: /^Asleep .* not interpolated$/ }).first(),
      route
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/${route}-sleep-gap-${info.project.name}.png`,
      fullPage: true,
    });
  }
});

test("power stack and core heatmap label an open pause", async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  // At the 15m default the paused scenario's open gap, which started 2
  // minutes ago, is inside both charts.
  for (const route of ["power", "cpu"]) {
    await page.goto(
      `/?window=dashboard&route=/dashboard/${route}&theme=dark&ticks=0&scenario=paused`
    );
    await expect(
      page.getByRole("note", { name: "Paused" }).first(),
      route
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/${route}-paused-${info.project.name}.png`,
      fullPage: true,
    });
  }
});

test("power page without a battery drops the battery section", async ({
  page,
}) => {
  await page.goto(
    "/?window=dashboard&route=/dashboard/power&ticks=0&scenario=no-battery"
  );
  await expect(page.getByText("From power adapter")).toBeVisible();
  await expect(page.getByText("Battery, last 24 hours")).toHaveCount(0);
});
