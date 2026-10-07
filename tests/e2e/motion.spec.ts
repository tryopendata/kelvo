import { expect, type Page, test } from "@playwright/test";

/**
 * One-shot motion (motion.md "Choreography") on the mock transport with the
 * tick stopped (`ticks=0`), so the only animations are the entrances:
 *
 * - a page's sections lift in on load and again on navigation, and nothing is
 *   left running or stuck invisible afterwards;
 * - under reduced motion nothing animates and everything is visible at once;
 * - Low Power Mode engages Performance mode, which stops all motion.
 */

/** Record the name of every CSS animation that starts, from page load on. */
async function recordAnimations(page: Page) {
  await page.addInitScript(() => {
    const names: string[] = [];
    (window as unknown as { __anims: string[] }).__anims = names;
    document.addEventListener("animationstart", (e) =>
      names.push(e.animationName)
    );
  });
}

const started = (page: Page) =>
  page.evaluate(() => (window as unknown as { __anims: string[] }).__anims);

/** Every entering element: the route root's children and CardGrid's cells. */
const sectionOpacities = (page: Page) =>
  page.evaluate(() =>
    Array.from(
      document.querySelectorAll("main > * > *, main [data-stagger] > *")
    ).map((el) => getComputedStyle(el).opacity)
  );

const running = (page: Page) =>
  page.evaluate(
    () =>
      document.getAnimations().filter((a) => a.playState === "running").length
  );

const openOverview = async (page: Page) => {
  await page.setViewportSize({ width: 1280, height: 833 });
  await page.goto("/?window=dashboard&theme=dark&ticks=0");
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
};

test("Overview sections and cards lift in, then settle with nothing running", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await recordAnimations(page);
  await openOverview(page);

  await expect
    .poll(
      async () => (await started(page)).filter((n) => /lift/.test(n)).length
    )
    .toBeGreaterThan(6);
  await expect.poll(() => running(page), { timeout: 2000 }).toBe(0);
  for (const opacity of await sectionOpacities(page)) expect(opacity).toBe("1");
});

test("navigating replays the page entrance and slides the sidebar pill", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await recordAnimations(page);
  await openOverview(page);
  await expect.poll(() => running(page), { timeout: 2000 }).toBe(0);

  const pill = page.locator("[data-sidebar-pill]");
  const before = await pill.evaluate((el) => el.style.transform);
  const liftCount = async () =>
    (await started(page)).filter((n) => /lift/.test(n)).length;
  const lifts = await liftCount();

  await page.locator("[data-sidebar-item][href='/dashboard/cpu']").click();
  await expect(
    page.getByRole("heading", { name: "CPU", exact: true })
  ).toBeVisible();
  await expect.poll(liftCount).toBeGreaterThan(lifts);
  await expect
    .poll(() => pill.evaluate((el) => el.style.transform))
    .not.toBe(before);
  // The first placement snaps with `transition: none`; it must hand the
  // transition back, or every later move would snap too.
  expect(
    await pill.evaluate((el) => getComputedStyle(el).transitionDuration)
  ).not.toBe("0s");
  await expect.poll(() => running(page), { timeout: 2000 }).toBe(0);
  for (const opacity of await sectionOpacities(page)) expect(opacity).toBe("1");
});

test("live ticks never replay an entrance", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await recordAnimations(page);
  await page.setViewportSize({ width: 1280, height: 833 });
  await page.goto("/?window=dashboard&theme=dark");
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
  await expect.poll(() => running(page), { timeout: 2000 }).toBe(0);

  await page.evaluate(() => {
    (window as unknown as { __anims: string[] }).__anims.length = 0;
  });
  // Three ticks of fresh values re-render every card.
  await page.waitForTimeout(3000);
  expect((await started(page)).filter((n) => /lift|fade|swap/.test(n))).toEqual(
    []
  );
  for (const opacity of await sectionOpacities(page)) expect(opacity).toBe("1");
});

test("reduced motion: nothing animates and every section is visible at once", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await recordAnimations(page);
  await openOverview(page);

  for (const opacity of await sectionOpacities(page)) expect(opacity).toBe("1");
  await page.locator("[data-sidebar-item][href='/dashboard/cpu']").click();
  await expect(
    page.getByRole("heading", { name: "CPU", exact: true })
  ).toBeVisible();
  for (const opacity of await sectionOpacities(page)) expect(opacity).toBe("1");
  expect(await started(page)).toEqual([]);
});

test("Low Power Mode engages Performance mode and stops all motion", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.setViewportSize({ width: 1280, height: 833 });
  await page.goto(
    "/?window=dashboard&theme=dark&ticks=0&scenario=low-power-mode"
  );
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-performance", "");
  const tokens = await page.evaluate(() => {
    const s = getComputedStyle(document.documentElement);
    return {
      tick: s.getPropertyValue("--motion-tick").trim(),
      count: s.getPropertyValue("--motion-count").trim(),
      enter: s.getPropertyValue("--motion-enter").trim(),
      crossfade: s.getPropertyValue("--motion-crossfade").trim(),
    };
  });
  expect(tokens).toEqual({
    tick: "0ms",
    count: "0ms",
    enter: "0ms",
    crossfade: "0ms",
  });
});
