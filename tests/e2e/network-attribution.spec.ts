import { expect, type Page, test } from "@playwright/test";
import { burstWindows, NET_BURSTS } from "../../src/core/mock/net-apps";

/**
 * Network attribution (D-089) on the mock transport: drag across
 * the mock's Docker Desktop burst, see Docker Desktop first in the Apps
 * table, and keep the selection pinned once it is off the chart.
 * Screenshots land in test-results/screens/ for the visual review; they
 * are not compared against a baseline.
 */

function trackConsoleErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  page.on("pageerror", (err) => errors.push(err.message));
  return errors;
}

/** The default chart window, 15m (D-091). */
const WINDOW_MS = 900_000;
/** The window switched to once a burst is selected: it starts after the burst. */
const SHORT_MS = 300_000;
const docker = NET_BURSTS.find((b) => b.app === "Docker Desktop");

test("drag across the Docker spike, then keep it pinned off the chart", async ({
  page,
}, info) => {
  if (!docker) throw new Error("the mock has no Docker Desktop burst");
  const errors = trackConsoleErrors(page);
  await page.setViewportSize({ width: 1280, height: 1100 });
  // ?ticks=0: the chart ends where the mock started, so burst times taken
  // relative to the start (startMs 0) place it on the 15m chart.
  await page.goto(
    "/?window=dashboard&route=/dashboard/network&theme=dark&ticks=0"
  );
  // The newest burst 5 to 15 minutes back, so it leaves the chart at 5m.
  const burst = burstWindows(docker, 0, -WINDOW_MS, -SHORT_MS).at(-1);
  if (!burst) throw new Error("no Docker burst 5 to 15 minutes back");
  const table = page.getByRole("table", { name: "Network by app" });
  await expect(table).toBeVisible();
  // Rows older than two minutes arrive as earlier chunks: drag once the
  // whole 15m, burst included, is drawn.
  await expect(
    page.getByRole("img", { name: /Drag to select/ }).locator("[data-gap]")
  ).toHaveCount(0);

  const brush = page.getByRole("slider", { name: "Select a time range" });
  const box = await brush.boundingBox();
  if (!box) throw new Error("brush has no box");
  const x = (t: number) => box.x + ((t + WINDOW_MS) / WINDOW_MS) * box.width;
  const y = box.y + box.height / 2;
  // The first part of the burst: snapped to 10 s, it ends before the 5m
  // chart starts, so switching to 5m scrolls it off.
  await page.mouse.move(x(burst[0] + 3000), y);
  await page.mouse.down();
  await page.mouse.move(x(burst[0] + 9000), y, { steps: 4 });
  await page.mouse.move(x(burst[0] + 14_000), y, { steps: 4 });
  await page.mouse.up();

  await expect(
    page.getByRole("heading", { name: /^Apps, selected \d+ s$/ })
  ).toBeVisible();
  await expect(table.getByRole("row").nth(1)).toContainText("Docker Desktop");
  const clear = page.getByRole("button", { name: "Clear selection" });
  await expect(clear).toBeVisible();
  await expect(page.getByTestId("brush-band")).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/network-attribution-selected-${info.project.name}.png`,
    fullPage: true,
  });

  await page.getByRole("radio", { name: "5m", exact: true }).click();
  await expect(page.getByTestId("selection-summary")).toContainText(
    "earlier than this chart"
  );
  await expect(page.getByTestId("brush-band")).toHaveCount(0);
  await expect(clear).toBeVisible();
  await expect(table.getByRole("row").nth(1)).toContainText("Docker Desktop");
  await page.screenshot({
    path: `test-results/screens/network-attribution-pinned-${info.project.name}.png`,
    fullPage: true,
  });

  await clear.click();
  await expect(
    page.getByRole("heading", { name: "Apps, last 5 minutes" })
  ).toBeVisible();
  await expect(
    page.getByText("Drag across the chart to see what used it.")
  ).toBeVisible();
  expect(errors).toEqual([]);
});
