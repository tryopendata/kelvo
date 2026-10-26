import { expect, type Page, test } from "@playwright/test";
import { chooseTimelinePreset } from "./helpers/timeline";

/**
 * v1.2 events (D-083, D-084) on the mock transport: the Timeline's event
 * pills, the Power chart's spike marker and the Settings alert switches,
 * in both themes. The clock is fixed at Sun Oct 4, 22:40, so the mock's
 * fan ramp reads 14:02.
 * Screenshots land in test-results/screens/ for the visual review.
 */
const THEMES = ["light", "dark"] as const;
const FIXED_NOW = new Date(2026, 9, 4, 22, 40);

function trackConsoleErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("console", (msg) => {
    if (msg.type() === "error") errors.push(msg.text());
  });
  page.on("pageerror", (err) => errors.push(err.message));
  return errors;
}

const shot = (name: string, theme: string, project: string) =>
  `test-results/screens/${name}-${theme}-${project}.png`;

const pills = (page: Page) =>
  page.getByRole("list", { name: "Timeline events" }).getByRole("button");

async function open(page: Page, route: string, theme: string) {
  await page.clock.setFixedTime(FIXED_NOW);
  await page.setViewportSize({ width: 1280, height: 860 });
  await page.goto(`/?window=dashboard&route=${route}&theme=${theme}&ticks=0`);
}

for (const theme of THEMES) {
  test.describe(`events (${theme})`, () => {
    test("24h event focus highlights only that event; a click investigates its episode", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await open(page, "/dashboard/timeline", theme);
      await chooseTimelinePreset(page, "Last 1 day");
      const event = page.getByRole("button", { name: /14:02 Fans ramped up/ });
      await expect(event).toBeVisible();
      await expect(page.locator("[data-event-band]")).toHaveCount(0);
      await event.focus();
      await expect(page.locator("[data-event-band]")).toHaveCount(1);
      await page.screenshot({
        path: shot("timeline-events-24h", theme, info.project.name),
      });
      await event.click();
      await expect(page.getByText(/^Recorded · /)).toBeVisible();
      await expect(page.getByText(/Loading recorded detail/)).toHaveCount(0);
      await expect(
        page.getByRole("slider", { name: "Timeline cursor" })
      ).toBeFocused();
      const focusedEvent = page.getByRole("button", {
        name: /14:02 Fans ramped up/,
      });
      await focusedEvent.click();
      await expect(
        page.getByRole("list", { name: "Events in group" })
      ).toBeVisible();
      await expect(
        page
          .getByRole("list", { name: "Events in group" })
          .getByRole("button", { name: /Fans ramped up/ })
      ).toBeVisible();
      await page.screenshot({
        path: shot("timeline-event-detail", theme, info.project.name),
      });
      expect(errors).toEqual([]);
    });

    test("7d clusters retain accessible counts and zoom into members", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await open(page, "/dashboard/timeline", theme);
      await chooseTimelinePreset(page, "Last 7 days");
      const groups = pills(page).filter({ hasText: /^\d+$/ });
      await expect(groups.first()).toBeVisible();
      const boxes = await pills(page).evaluateAll((els) =>
        els.map((el) => {
          const rect = el.getBoundingClientRect();
          return { left: rect.left, right: rect.right };
        })
      );
      for (let i = 1; i < boxes.length; i++)
        expect(boxes[i]?.left).toBeGreaterThanOrEqual(boxes[i - 1]?.right ?? 0);
      await page.screenshot({
        path: shot("timeline-events-7d", theme, info.project.name),
      });
      await groups.first().click();
      await expect(page.getByText(/^Recorded · /)).toBeVisible();
      await expect(page.getByText(/Loading recorded detail/)).toHaveCount(0);
      await expect(
        page.getByRole("slider", { name: "Timeline cursor" })
      ).toBeFocused();
      expect(errors).toEqual([]);
    });

    test("power chart marks the ANE spike", async ({ page }, info) => {
      const errors = trackConsoleErrors(page);
      await open(page, "/dashboard/power", theme);
      await expect(
        page.getByText("ANE 1.4 W · Photos face analysis")
      ).toBeVisible();
      await page.screenshot({
        path: shot("power-spike-annotation", theme, info.project.name),
        fullPage: true,
      });
      expect(errors).toEqual([]);
    });

    test("alert switches write through update_settings", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await open(page, "/dashboard/settings", theme);
      const hot = page.getByRole("switch", {
        name: "Alert when a process is above 200% CPU for 5 minutes",
      });
      const thermal = page.getByRole("switch", {
        name: "Alert when the thermal state is Serious or worse",
      });
      await expect(hot).not.toBeChecked();
      await expect(thermal).not.toBeChecked();
      await thermal.click();
      await expect(thermal).toBeChecked();
      await expect(hot).not.toBeChecked();
      await page.screenshot({
        path: shot("settings-alerts", theme, info.project.name),
        fullPage: true,
      });
      await thermal.click();
      await expect(thermal).not.toBeChecked();
      expect(errors).toEqual([]);
    });
  });
}
