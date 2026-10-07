import { expect, type Page, test } from "@playwright/test";

/**
 * Timeline 7d and 30d (plan v1.1 phase 1.1-A) on the mock
 * transport, whose history carries a sleep every night for 30 days, a
 * weekend away and two multi-hour afternoon holes. Screenshots land in
 * test-results/screens/ for the visual review (verification.md); they are
 * not compared against a baseline.
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

const shot = (name: string, theme: string, project: string) =>
  `test-results/screens/${name}-${theme}-${project}.png`;

/** Hatched gap bands drawn over the lanes, one per whole-host gap in range. */
async function bandCount(page: Page): Promise<number> {
  return page.getByRole("note").count();
}

async function openRange(page: Page, theme: string, span: "7d" | "30d") {
  await page.setViewportSize({ width: 1280, height: 860 });
  await page.goto(
    `/?window=dashboard&route=/dashboard/timeline&theme=${theme}&ticks=0`
  );
  await expect(page.getByRole("heading", { name: "Timeline" })).toBeVisible();
  await page.getByRole("radio", { name: span, exact: true }).click();
  await expect(
    page.getByText(
      span === "7d" ? "Last 7 days, ending" : "Last 30 days, ending"
    )
  ).toBeVisible();
  await expect(page.locator("[data-lane-plot]")).toHaveCount(6);
}

for (const theme of THEMES) {
  test.describe(`timeline long ranges (${theme})`, () => {
    test("7d draws day ticks, nightly sleep bands and merged buckets", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await openRange(page, theme, "7d");
      // Six nights before last night's, plus last night and the afternoon
      // Kelvo was not running: whole-host bands, one per gap.
      await expect.poll(() => bandCount(page)).toBeGreaterThanOrEqual(7);
      // Day ticks along the axis ("Tue 29"): 7 midnights, less the newest
      // when it falls in the last tenth of the range, by the "now" label.
      const dayTicks = page.getByText(
        /^(Sun|Mon|Tue|Wed|Thu|Fri|Sat) \d{1,2}$/
      );
      await expect.poll(() => dayTicks.count()).toBeGreaterThanOrEqual(6);
      expect(await dayTicks.count()).toBeLessThanOrEqual(7);

      const cursor = page.getByRole("slider", { name: "Timeline cursor" });
      const box = await cursor.boundingBox();
      if (!box) throw new Error("no cursor overlay");
      await page.mouse.move(box.x + box.width * 0.93, box.y + box.height * 0.3);
      const tooltip = page.getByRole("tooltip");
      await expect(tooltip).toBeVisible();
      // 1280 px leaves an ~820 px plot: 10-minute buckets at 2 per pixel.
      await expect(tooltip.getByText("10 min avg")).toBeVisible();
      await expect(tooltip.getByText("Top processes then")).toBeVisible();
      await expect(tooltip.getByText("Xcode").first()).toBeVisible();
      await page.screenshot({
        path: shot("timeline-7d", theme, info.project.name),
        fullPage: true,
      });
      expect(errors).toEqual([]);
    });

    test("30d draws week ticks, a month of sleep bands and steps back", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await openRange(page, theme, "30d");
      // 28 nights plus the weekend away, last night and two afternoons.
      await expect.poll(() => bandCount(page)).toBeGreaterThanOrEqual(25);
      // The weekend away is one band, not two nights with data between.
      await expect(
        page.getByRole("note", { name: /^Asleep 1d 10h/ })
      ).toHaveCount(1);
      await expect(page.getByRole("note", { name: "Paused" })).toHaveCount(1);

      const cursor = page.getByRole("slider", { name: "Timeline cursor" });
      const box = await cursor.boundingBox();
      if (!box) throw new Error("no cursor overlay");
      await page.mouse.move(box.x + box.width * 0.93, box.y + box.height * 0.3);
      const tooltip = page.getByRole("tooltip");
      await expect(tooltip.getByText("30 min avg")).toBeVisible();
      // Processes for a 30-minute bucket: asked at its middle.
      await expect(tooltip.getByText("Xcode").first()).toBeVisible();
      await page.screenshot({
        path: shot("timeline-30d", theme, info.project.name),
        fullPage: true,
      });

      await page.mouse.move(0, 0);
      await page.getByRole("button", { name: "Previous range" }).click();
      await expect(page.getByText(/^30 days, /)).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Next range" })
      ).toBeVisible();
      expect(errors).toEqual([]);
    });
  });
}
