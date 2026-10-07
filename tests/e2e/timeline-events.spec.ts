import { expect, type Page, test } from "@playwright/test";

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
  page
    .getByRole("list", { name: "Events, sleep and wake" })
    .getByRole("button");

async function open(page: Page, route: string, theme: string) {
  await page.clock.setFixedTime(FIXED_NOW);
  await page.setViewportSize({ width: 1280, height: 860 });
  await page.goto(`/?window=dashboard&route=${route}&theme=${theme}&ticks=0`);
}

for (const theme of THEMES) {
  test.describe(`events (${theme})`, () => {
    test("24h pill, click moves the crosshair", async ({ page }, info) => {
      const errors = trackConsoleErrors(page);
      await open(page, "/dashboard/timeline", theme);
      const fans = pills(page).filter({ hasText: "Fans ramped up" });
      await expect(fans).toHaveText(
        "14:02 Fans ramped up · kernel_task + Xcode build"
      );
      await page.screenshot({
        path: shot("timeline-events-24h", theme, info.project.name),
        fullPage: true,
      });

      // The pill's dot sits on the event's time: its right edge is 4 px
      // past the band the event shades.
      const band = await page.locator("[data-event-band]").nth(1).boundingBox();
      const pill = await fans.boundingBox();
      if (!band || !pill) throw new Error("no band or pill");
      expect(
        Math.abs(pill.x + pill.width - (band.x + band.width) - 4)
      ).toBeLessThan(5);

      await fans.click();
      const cursor = page.getByRole("slider", { name: "Timeline cursor" });
      await expect(cursor).toBeFocused();
      await expect(cursor).toHaveAttribute("aria-valuetext", /14:02:00$/);
      await expect(page.getByText("Top processes then")).toBeVisible();
      await page.screenshot({
        path: shot("timeline-events-24h-cursor", theme, info.project.name),
        fullPage: true,
      });
      // The arrow keys step on from the event.
      await page.keyboard.press("ArrowRight");
      await expect(cursor).toHaveAttribute("aria-valuetext", /14:03:00$/);
      expect(errors).toEqual([]);
    });

    test("7d crowds the day's events into +N, never overlapping", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await open(page, "/dashboard/timeline", theme);
      await page.getByRole("radio", { name: "7d", exact: true }).click();
      await expect(pills(page).filter({ hasText: /\+\d/ })).not.toHaveCount(0);
      const boxes = await pills(page).evaluateAll((els) =>
        els.map((el) => {
          const r = el.getBoundingClientRect();
          return { l: r.left, r: r.right, t: r.top };
        })
      );
      for (const [i, a] of boxes.entries()) {
        for (const b of boxes.slice(i + 1)) {
          const sameRow = Math.abs(a.t - b.t) < 1;
          expect(sameRow && a.l < b.r && b.l < a.r).toBe(false);
        }
      }
      await page.screenshot({
        path: shot("timeline-events-7d", theme, info.project.name),
        fullPage: true,
      });
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
