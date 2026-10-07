import { readFileSync } from "node:fs";
import { type CDPSession, expect, type Page, test } from "@playwright/test";

/**
 * Frontend performance gate (plan 7; thresholds in `perf-budget.json`).
 * Opens the popover, the Overview, and the CPU, GPU and Power pages on the
 * 1 h chart window (the longest live charts: a full hour of ring, bucketed,
 * and per-key stats over the hour on GPU and Power) on the mock transport
 * ticking at 1 Hz with about 800 processes, lets them warm up, then samples Chromium's own counters (CDP
 * `Performance.getMetrics`) over a window and asserts:
 *
 * - script + layout + style-recalc time per second of wall time stays under
 *   the budget for that screen;
 * - no long task (over 50 ms) after warm-up.
 *
 * Chromium only: CDP is not available on WebKit. The numbers are the
 * renderer's main-thread time for the page, not the app's coalition CPU; the
 * gate catches a render path that got more expensive. Raising a threshold
 * needs a decision entry (rules/frontend/testing.md).
 */

interface ScreenBudget {
  /** Script + layout + style recalc, ms per second of wall time. */
  mainThreadMsPerSec: number;
}

interface FrontendBudget {
  warmupMs: number;
  sampleMs: number;
  longTaskMs: number;
  popover: ScreenBudget;
  overview: ScreenBudget;
  cpu1h: ScreenBudget;
  gpu1h: ScreenBudget;
  power1h: ScreenBudget;
  dashboardOpen: { longTaskMs: number; fillWithinMs: number };
  hiddenResume: { longTaskMs: number };
}

const budget: FrontendBudget = JSON.parse(
  readFileSync(new URL("../../perf-budget.json", import.meta.url), "utf8")
).frontend;

/** Seconds of main-thread work, cumulative since `Performance.enable`. */
const METRICS = ["ScriptDuration", "LayoutDuration", "RecalcStyleDuration"];

async function metrics(cdp: CDPSession): Promise<Record<string, number>> {
  const { metrics } = await cdp.send("Performance.getMetrics");
  return Object.fromEntries(metrics.map((m) => [m.name, m.value]));
}

interface LongTask {
  start: number;
  duration: number;
}

/** Record long tasks from page load on, in `window.__longTasks`. */
async function observeLongTasks(page: Page) {
  await page.addInitScript(() => {
    const tasks: LongTask[] = [];
    (window as unknown as { __longTasks: LongTask[] }).__longTasks = tasks;
    new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        tasks.push({ start: e.startTime, duration: e.duration });
      }
    }).observe({ type: "longtask", buffered: true });
  });
}

const timeOrigin = (page: Page) => page.evaluate(() => performance.timeOrigin);

async function measure(page: Page, url: string, ready: () => Promise<void>) {
  await observeLongTasks(page);
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("Performance.enable", { timeDomain: "timeTicks" });
  await page.goto(url);
  // The dev server reloads every open page when another spec makes it
  // optimize a new dependency. A reload resets the counters, so a sample
  // that spans one is thrown away and taken again.
  for (let attempt = 0; ; attempt++) {
    await ready();
    await page.waitForTimeout(budget.warmupMs);
    const origin = await timeOrigin(page);
    const sample = await sampleOnce(page, cdp);
    if ((await timeOrigin(page)) === origin || attempt === 2) return sample;
    console.log("[perf] the page reloaded during the sample; measuring again");
  }
}

async function sampleOnce(page: Page, cdp: CDPSession) {
  const warmEnd = await page.evaluate(() => performance.now());
  const before = await metrics(cdp);
  await page.waitForTimeout(budget.sampleMs);
  const after = await metrics(cdp);
  const seconds = (after.Timestamp ?? 0) - (before.Timestamp ?? 0);
  const perSec: Record<string, number> = {};
  let total = 0;
  for (const name of METRICS) {
    const ms = (((after[name] ?? 0) - (before[name] ?? 0)) * 1000) / seconds;
    perSec[name] = Math.round(ms * 100) / 100;
    total += ms;
  }
  const longTasks = await page.evaluate(
    ({ from, over }) =>
      (window as unknown as { __longTasks: LongTask[] }).__longTasks.filter(
        (t) => t.start >= from && t.duration > over
      ),
    { from: warmEnd, over: budget.longTaskMs }
  );
  return {
    seconds: Math.round(seconds * 10) / 10,
    perSec,
    mainThreadMsPerSec: Math.round(total * 100) / 100,
    longTasks,
  };
}

test.describe("performance gate", () => {
  test.skip(
    ({ browserName }) => browserName !== "chromium",
    "CDP metrics are Chromium-only"
  );
  // Three samples at most per measure, and the Performance mode case measures twice.
  test.setTimeout(6 * (budget.warmupMs + budget.sampleMs) + 30_000);

  test("popover at 1 Hz", async ({ page }) => {
    await page.setViewportSize({ width: 360, height: 760 });
    const r = await measure(page, "/?window=popover&theme=dark", async () => {
      await expect(page.getByText("Open dashboard")).toBeVisible();
    });
    console.log(`[perf] popover ${JSON.stringify(r)}`);
    expect(r.seconds).toBeGreaterThan(budget.sampleMs / 1000 - 1);
    expect(r.longTasks).toEqual([]);
    expect(r.mainThreadMsPerSec).toBeLessThan(
      budget.popover.mainThreadMsPerSec
    );
  });

  test("Overview at 1 Hz", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 833 });
    const r = await measure(page, "/?window=dashboard&theme=dark", async () => {
      await expect(
        page.getByRole("heading", { name: "Overview", exact: true })
      ).toBeVisible();
    });
    console.log(`[perf] overview ${JSON.stringify(r)}`);
    expect(r.seconds).toBeGreaterThan(budget.sampleMs / 1000 - 1);
    expect(r.longTasks).toEqual([]);
    expect(r.mainThreadMsPerSec).toBeLessThan(
      budget.overview.mainThreadMsPerSec
    );
  });

  /**
   * Performance mode (D-088) against plain, same test so both halves share
   * the machine's load: frames every 2 s and no motion must cost less main
   * thread on the Overview. Only the difference is asserted; the plain
   * Overview's own threshold is the case above.
   */
  test("Overview in Performance mode costs less than plain", async ({
    browser,
  }) => {
    const overview = async (scenario: string) => {
      const page = await browser.newPage({
        viewport: { width: 1280, height: 833 },
      });
      const r = await measure(
        page,
        `/?window=dashboard&theme=dark&scenario=${scenario}`,
        async () => {
          await expect(
            page.getByRole("heading", { name: "Overview", exact: true })
          ).toBeVisible();
        }
      );
      await page.close();
      return r;
    };
    // On AC, so the interval stays 1 s and only Performance mode differs.
    const plain = await overview("no-battery");
    const perf = await overview("no-battery,performance-mode");
    console.log(
      `[perf] overview plain ${plain.mainThreadMsPerSec}, performance mode ${perf.mainThreadMsPerSec} ms/s`
    );
    expect(perf.longTasks).toEqual([]);
    expect(perf.mainThreadMsPerSec).toBeLessThan(plain.mainThreadMsPerSec);
  });

  test("CPU page, 1 h window, at 1 Hz", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 833 });
    const r = await measure(
      page,
      "/?window=dashboard&route=/dashboard/cpu&theme=dark",
      async () => {
        await page.getByRole("radio", { name: "1h", exact: true }).click();
        await expect(
          page.getByRole("img", { name: /CPU total and system, last hour/ })
        ).toBeVisible();
      }
    );
    console.log(`[perf] cpu1h ${JSON.stringify(r)}`);
    expect(r.seconds).toBeGreaterThan(budget.sampleMs / 1000 - 1);
    expect(r.longTasks).toEqual([]);
    expect(r.mainThreadMsPerSec).toBeLessThan(budget.cpu1h.mainThreadMsPerSec);
  });

  test("GPU page, 1 h window, at 1 Hz", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 833 });
    const r = await measure(
      page,
      "/?window=dashboard&route=/dashboard/gpu&theme=dark&chart_window=1h",
      async () => {
        await expect(
          page.getByRole("radio", { name: "1h", exact: true })
        ).toBeChecked();
        await expect(
          page.getByRole("img", { name: /GPU utilization, last hour/ })
        ).toBeVisible();
      }
    );
    console.log(`[perf] gpu1h ${JSON.stringify(r)}`);
    expect(r.seconds).toBeGreaterThan(budget.sampleMs / 1000 - 1);
    expect(r.longTasks).toEqual([]);
    expect(r.mainThreadMsPerSec).toBeLessThan(budget.gpu1h.mainThreadMsPerSec);
  });

  test("Power page, 1 h window, at 1 Hz", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 833 });
    const r = await measure(
      page,
      "/?window=dashboard&route=/dashboard/power&theme=dark&chart_window=1h",
      async () => {
        await expect(
          page.getByRole("radio", { name: "1h", exact: true })
        ).toBeChecked();
        await expect(
          page.getByText("Power by component, last hour")
        ).toBeVisible();
      }
    );
    console.log(`[perf] power1h ${JSON.stringify(r)}`);
    expect(r.seconds).toBeGreaterThan(budget.sampleMs / 1000 - 1);
    expect(r.longTasks).toEqual([]);
    expect(r.mainThreadMsPerSec).toBeLessThan(
      budget.power1h.mainThreadMsPerSec
    );
  });

  /**
   * First open (D-066): the channel sends the last 2 minutes, then the rest
   * of the hour as `backfill_earlier` chunks of 600 rows. On the wide
   * layout (170 series, a full Mac's count) that is 3,600 rows of 170
   * values. Prepending them must not block the main thread, and the 1 h
   * chart must end up drawing the whole hour.
   */
  test("dashboard open, backfilling the hour", async ({ page }) => {
    const open = budget.dashboardOpen;
    await page.setViewportSize({ width: 1280, height: 833 });
    await observeLongTasks(page);
    await page.goto(
      "/?window=dashboard&route=/dashboard/cpu&theme=dark&scenario=wide-layout"
    );
    const oneHour = page.getByRole("radio", { name: "1h", exact: true });
    await expect(oneHour).toBeVisible();
    // Module loading and first mount are before this; the backfill is after.
    const from = await page.evaluate(() => performance.now());
    await oneHour.click();
    const chart = page.getByRole("img", {
      name: /CPU total and system, last hour/,
    });
    await expect(chart).toBeVisible();

    // Share of the chart's width the cpu.total line spans.
    const span = () =>
      chart.evaluate((svg) => {
        const line = svg.querySelector<SVGGraphicsElement>(
          '[data-series="cpu.total"] path[data-line]'
        );
        const width = (svg as SVGSVGElement).viewBox.baseVal.width;
        return line && width > 0 ? line.getBBox().width / width : 0;
      });
    await expect
      .poll(span, { timeout: open.fillWithinMs, intervals: [250] })
      .toBeGreaterThan(0.95);
    const filledAt = await page.evaluate(() => performance.now());
    // A couple of ticks past the fill, so the last chunk's render is in.
    await page.waitForTimeout(2000);

    const tasks = await page.evaluate(
      ({ from: f }) =>
        (window as unknown as { __longTasks: LongTask[] }).__longTasks.filter(
          (t) => t.start >= f
        ),
      { from }
    );
    const r = {
      fillMs: Math.round(filledAt - from),
      longest: Math.round(Math.max(0, ...tasks.map((t) => t.duration))),
      longTasks: tasks.length,
    };
    console.log(`[perf] dashboardOpen ${JSON.stringify(r)}`);
    expect(tasks.filter((t) => t.duration > open.longTaskMs)).toEqual([]);
  });

  /**
   * Shown after half an hour hidden (review #5): the channel resumes with
   * the 1,800 rows the window missed, in `backfill` messages of at most 600
   * rows on the wide layout, one per task. Applying them must not block the
   * main thread, and the 1 h chart must come out without a hole.
   */
  test("dashboard shown after half an hour hidden", async ({ page }) => {
    const longTaskMs = budget.hiddenResume.longTaskMs;
    await page.setViewportSize({ width: 1280, height: 833 });
    await observeLongTasks(page);
    await page.goto(
      "/?window=dashboard&route=/dashboard/cpu&theme=dark&scenario=wide-layout,hidden-resume"
    );
    await page.getByRole("radio", { name: "1h", exact: true }).click();
    const chart = page.getByRole("img", {
      name: /CPU total and system, last hour/,
    });
    await expect(chart).toBeVisible();

    // The mock hides the window on its eleventh tick, samples half an hour,
    // shows it again and marks `kelvo:resume`.
    const resumedAt = () =>
      page.evaluate(
        () => performance.getEntriesByName("kelvo:resume")[0]?.startTime ?? null
      );
    await expect.poll(resumedAt, { timeout: 20_000 }).not.toBeNull();
    const from = (await resumedAt()) ?? 0;
    // The chunks, the frame behind them and a few ticks of render.
    await page.waitForTimeout(3000);

    // Subpaths of the cpu.total line: a dropped chunk would leave a hole.
    const subpaths = await chart.evaluate(
      (svg) =>
        svg
          .querySelector('[data-series="cpu.total"] path[data-line]')
          ?.getAttribute("d")
          ?.match(/M/g)?.length ?? 0
    );
    const tasks = await page.evaluate(
      ({ from: f }) =>
        (window as unknown as { __longTasks: LongTask[] }).__longTasks.filter(
          (t) => t.start >= f
        ),
      { from }
    );
    const r = {
      longest: Math.round(Math.max(0, ...tasks.map((t) => t.duration))),
      longTasks: tasks.length,
      subpaths,
    };
    console.log(`[perf] hiddenResume ${JSON.stringify(r)}`);
    expect(subpaths).toBe(1);
    expect(tasks.filter((t) => t.duration > longTaskMs)).toEqual([]);
  });
});
