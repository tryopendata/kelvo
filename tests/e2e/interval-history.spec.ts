import { expect, type Page, test } from "@playwright/test";

/**
 * Sampling interval and history size limit (plan 4.15, D-059, D-061): the
 * Settings controls, the store notices on Settings and the Timeline, and
 * the popover's charts following a 30 s interval. Screenshots land in
 * test-results/screens/ for the visual review.
 */

function trackConsoleErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  page.on("pageerror", (err) => errors.push(err.message));
  return errors;
}

const shot = (page: Page, name: string, project: string) =>
  page.screenshot({
    path: `test-results/screens/${name}-${project}.png`,
    fullPage: true,
  });

test("Settings: interval, size limit and history notices", async ({
  page,
}, info) => {
  const errors = trackConsoleErrors(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(
    "/?window=dashboard&route=/dashboard/settings&theme=dark&ticks=0&scenario=low-disk,history-trimmed"
  );
  await expect(
    page.getByText(/History paused: disk almost full/)
  ).toBeVisible();
  await expect(
    page.getByText(/History trimmed to stay under 150 MB/)
  ).toBeVisible();

  await page.getByRole("radio", { name: "30s" }).click();
  await expect(page.getByRole("radio", { name: "30s" })).toHaveAttribute(
    "data-state",
    "on"
  );
  await expect(page.getByText("to 60s")).toBeVisible();

  await page.getByRole("combobox", { name: "Keep history" }).click();
  // Each option shows what it costs. With 15-minute buckets past 7 days
  // (D-076), 90 days fits under 150 MB at the mock's series count; the
  // "Limited to" copy is covered by the history-projection unit tests.
  await expect(
    page.getByRole("option", { name: /^7 days about/ })
  ).toBeVisible();
  await expect(
    page.getByRole("option", { name: /^90 days about \d+ MB/ })
  ).toBeVisible();
  await page.getByRole("option", { name: /^90 days/ }).click();
  await expect(page.getByText(/Limited to about/)).toBeHidden();
  await shot(page, "settings-history-limit", info.project.name);
  expect(errors).toEqual([]);
});

test("Timeline warns about the low-disk pause, not an old trim", async ({
  page,
}, info) => {
  const errors = trackConsoleErrors(page);
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(
    "/?window=dashboard&route=/dashboard/timeline&theme=dark&ticks=0&scenario=low-disk,history-trimmed"
  );
  await expect(page.getByRole("heading", { name: "Timeline" })).toBeVisible();
  await expect(
    page.getByText(/History paused: disk almost full/)
  ).toBeVisible();
  // The trim is 20 days back; a 24 h range never reaches it.
  await expect(page.getByText(/History trimmed/)).toHaveCount(0);
  await shot(page, "timeline-low-disk", info.project.name);
  expect(errors).toEqual([]);
});

test("Timeline and the battery card show the unavailable banner", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(
    "/?window=dashboard&route=/dashboard/timeline&theme=dark&ticks=0&scenario=history-unavailable"
  );
  await expect(
    page.getByText("History is unavailable. Live values still work.")
  ).toHaveCount(1);
  await page.goto(
    "/?window=dashboard&route=/dashboard/battery&theme=dark&ticks=0&scenario=history-unavailable"
  );
  await expect(
    page.getByText("History is unavailable. Live values still work.")
  ).toBeVisible();
});

test("popover charts cover 30 minutes at a 30 s interval", async ({
  page,
}, info) => {
  const errors = trackConsoleErrors(page);
  await page.setViewportSize({ width: 360, height: 760 });
  await page.goto("/?window=popover&theme=dark&ticks=0&interval=30000");
  await expect(page.getByText("Open dashboard")).toBeVisible();
  await expect(page.getByLabel(/^CPU, last 30 minutes/)).toBeVisible();
  await expect(page.getByText("30m", { exact: true })).toBeVisible();
  await shot(page, "popover-30s", info.project.name);
  expect(errors).toEqual([]);
});

test("module windows too short for the interval are disabled", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(
    "/?window=dashboard&route=/dashboard/cpu&theme=dark&ticks=0&interval=60000&chart_window=5m"
  );
  const control = page.getByRole("radiogroup", { name: "Window" });
  await expect(
    control.getByRole("radio", { name: "5m", exact: true })
  ).toBeDisabled();
  await expect(
    control.getByRole("radio", { name: "15m", exact: true })
  ).toBeEnabled();
  // A saved 5m shows as the shortest window still offered, unsaved.
  await expect(
    control.getByRole("radio", { name: "15m", exact: true })
  ).toHaveAttribute("data-state", "on");
});
