import { expect, type Page, test } from "@playwright/test";

/**
 * The Timeline's 30-day heatmap and Export CSV (plan v1.1 phases 1.1-B and
 * 1.1-C) on the mock transport. The
 * screenshots land in test-results/screens/ for the visual review; they are
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

const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];
const pad = (n: number) => String(n).padStart(2, "0");

/** Local `h:m` on the day `n` days before today (the browser shares this zone). */
function daysAgo(n: number, h: number, m = 0): Date {
  const t = new Date();
  return new Date(t.getFullYear(), t.getMonth(), t.getDate() - n, h, m);
}

/** The subtitle's "Sun Oct 4 · 14:00". */
const dayClock = (d: Date) =>
  `${WEEKDAYS[d.getDay()]} ${MONTHS[d.getMonth()]} ${d.getDate()} · ${pad(d.getHours())}:${pad(d.getMinutes())}`;

const shot = (name: string, theme: string, project: string) =>
  `test-results/screens/${name}-${theme}-${project}.png`;

async function open(page: Page, theme: string) {
  await page.setViewportSize({ width: 1280, height: 860 });
  await page.goto(
    `/?window=dashboard&route=/dashboard/timeline&theme=${theme}&ticks=0`
  );
  await expect(page.getByRole("heading", { name: "Timeline" })).toBeVisible();
  const card = page.getByRole("region", { name: "30-day heatmap" });
  await expect(card.getByRole("gridcell")).toHaveCount(720);
  await expect(
    card.getByRole("gridcell", { name: /average CPU \d+%$/ }).first()
  ).toBeAttached();
  return card;
}

/** Canvas pixels with any paint: the cells were drawn, not just laid out. */
async function paintedPixels(page: Page): Promise<number> {
  return page.locator("[data-heatmap-canvas]").evaluate((c) => {
    const canvas = c as HTMLCanvasElement;
    const ctx = canvas.getContext("2d");
    if (!ctx) return 0;
    const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
    let n = 0;
    for (let i = 3; i < data.length; i += 4) if ((data[i] ?? 0) > 0) n++;
    return n;
  });
}

for (const theme of THEMES) {
  test.describe(`timeline heatmap (${theme})`, () => {
    test("draws 30 days by hour and switches to temperature", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      const card = await open(page, theme);
      await expect(card.getByRole("heading")).toHaveText(
        "Last 30 days, by hour"
      );
      await expect(card.getByRole("row")).toHaveCount(30);
      await expect(
        card.getByRole("gridcell", { name: /no samples$/ }).first()
      ).toBeAttached();
      await expect(card.getByText("0%", { exact: true })).toBeVisible();
      await expect(card.getByText("80%+")).toBeVisible();
      await expect.poll(() => paintedPixels(page)).toBeGreaterThan(10_000);
      await card.scrollIntoViewIfNeeded();
      await page.screenshot({
        path: shot("timeline-heatmap", theme, info.project.name),
        fullPage: true,
      });
      await card.screenshot({
        path: shot("timeline-heatmap-card", theme, info.project.name),
      });

      await card.getByRole("radio", { name: "Temperature" }).click();
      await expect(
        card
          .getByRole("gridcell", { name: /average temperature \d+ °C$/ })
          .first()
      ).toBeAttached();
      await expect(card.getByText("90 °C+")).toBeVisible();
      await card.screenshot({
        path: shot("timeline-heatmap-temp", theme, info.project.name),
      });
      expect(errors).toEqual([]);
    });

    test("a cell opens its hour; an old one opens 6 hours; Live returns", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      const card = await open(page, theme);
      // Yesterday 14:00: within the minutes, so the hour itself.
      await card.locator('[data-cell="28-14"]').click();
      await expect(page.getByText(/^One hour, /)).toBeVisible();
      await expect(
        page.getByText(`${dayClock(daysAgo(1, 14))} – 15:00`, { exact: true })
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Next range" })
      ).toBeVisible();
      await expect(page.locator("[data-lane-plot]")).toHaveCount(6);

      // Keyboard: the cell keeps focus; up 20 rows is 20 days further back.
      const cell = card.locator('[data-cell="28-14"]');
      await cell.focus();
      for (let i = 0; i < 20; i++) await page.keyboard.press("ArrowUp");
      await expect(card.locator('[data-cell="8-14"]')).toBeFocused();
      await page.keyboard.press("Enter");
      await expect(page.getByText(/^6 hours, /)).toBeVisible();
      // 21 days back, centred on 14:00 to 15:00.
      await expect(
        page.getByText(`${dayClock(daysAgo(21, 11, 30))} – 17:30`, {
          exact: true,
        })
      ).toBeVisible();
      // Focus follows the view up to the Timeline.
      await expect(
        page.getByRole("heading", { name: "Timeline" })
      ).toBeFocused();
      // No preset is selected for the 6-hour window.
      await expect(page.getByRole("radio", { checked: true })).toHaveCount(1);
      const range = page.getByRole("radiogroup", { name: "Range" });
      await expect(range.getByRole("radio")).toHaveCount(4);
      await expect(range.getByRole("radio", { checked: true })).toHaveCount(0);
      await expect(range.locator("[data-state=on]")).toHaveCount(0);
      await page.screenshot({
        path: shot("timeline-6h", theme, info.project.name),
        animations: "disabled",
      });

      await page.getByRole("button", { name: "Live" }).click();
      await expect(page.getByText("Last 24 hours, ending")).toBeVisible();
      expect(errors).toEqual([]);
    });

    test("a 6h window stepped forward onto now follows Live at 24h", async ({
      page,
    }) => {
      const errors = trackConsoleErrors(page);
      const card = await open(page, theme);
      // 7 days ago at 00:00 is past the minutes, so 6 hours: 28 steps to now.
      await card.locator('[data-cell="22-0"]').click();
      await expect(page.getByText(/^6 hours, /)).toBeVisible();
      const next = page.getByRole("button", { name: "Next range" });
      for (let i = 0; i < 40 && (await next.isVisible()); i++) {
        await next.click();
      }
      await expect(next).toBeHidden();
      await expect(page.getByText("Last 24 hours, ending")).toBeVisible();
      await expect(
        page
          .getByRole("radiogroup", { name: "Range" })
          .getByRole("radio", { name: "24h", checked: true })
      ).toBeVisible();
      expect(errors).toEqual([]);
    });

    test("Export CSV saves the visible range and says where", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await open(page, theme);
      const button = page.getByRole("button", { name: "Export CSV" });
      await expect(button).toBeVisible();
      await page.screenshot({
        path: shot("timeline-header", theme, info.project.name),
        clip: { x: 0, y: 0, width: 1280, height: 120 },
      });
      await button.click();
      await expect(
        page.getByText(
          /^Exported [\d,]+ rows.* to \/Users\/mock\/Downloads\/kelvo-\d{4}-\d{2}-\d{2}-\d{4}-24h\.csv$/
        )
      ).toBeVisible();
      expect(errors).toEqual([]);
    });
  });
}
