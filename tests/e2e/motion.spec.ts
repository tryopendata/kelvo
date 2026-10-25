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

/** All running motion except the explicitly bounded live-value transitions. */
const unexpectedRunning = (page: Page) =>
  page.evaluate(() =>
    document
      .getAnimations()
      .filter((a) => a.playState === "running")
      .filter((a) => {
        if (!(a instanceof CSSTransition)) return true;
        const target = (a.effect as KeyframeEffect | null)?.target;
        if (!(target instanceof Element)) return true;
        const timing = a.effect?.getTiming();
        // CSS shortens an interrupted reversal; its declared token stays 150ms.
        const declared = getComputedStyle(target).transitionDuration.split(",");
        // RingGauge: each live reading tweens arc length and segment rotation.
        const ring =
          target.matches("main svg circle") &&
          target.classList.contains(
            "transition-[stroke-dashoffset,transform]"
          ) &&
          ["stroke-dashoffset", "transform"].includes(a.transitionProperty);
        // InlineBar's MeterTrack: each live reading tweens the fill scale.
        const bar =
          target.matches("main span.origin-left.transition-transform") &&
          a.transitionProperty === "transform";
        return !(
          timing?.iterations === 1 &&
          typeof timing.duration === "number" &&
          Number.isFinite(timing.duration) &&
          timing.duration > 0 &&
          timing.duration <= 150 &&
          declared.every((value) => value.trim() === "0.15s") &&
          (ring || bar)
        );
      })
      .map((a) =>
        a instanceof CSSAnimation
          ? a.animationName
          : a instanceof CSSTransition
            ? a.transitionProperty
            : a.constructor.name
      )
  );

const openOverview = async (page: Page) => {
  await page.setViewportSize({ width: 1280, height: 833 });
  await page.goto("/?window=dashboard&theme=dark&ticks=0");
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
};

async function reverseRing(page: Page, duration: number | null = null) {
  await openOverview(page);
  await expect.poll(() => running(page), { timeout: 2000 }).toBe(0);
  const original = await page.evaluate(async (duration) => {
    const circle = Array.from(document.querySelectorAll("main circle")).find(
      (el) => el.classList.contains("transition-[stroke-dashoffset,transform]")
    );
    if (!(circle instanceof SVGElement)) throw new Error("No ring circle");
    circle.setAttribute("data-motion-test-ring", "");
    if (duration !== null) circle.style.transitionDuration = `${duration}ms`;
    const original = circle.getAttribute("stroke-dashoffset");
    if (original === null) throw new Error("No ring offset");
    circle.setAttribute("stroke-dashoffset", String(Number(original) + 50));
    getComputedStyle(circle).strokeDashoffset;
    const forward = circle
      .getAnimations()
      .find(
        (a) =>
          a instanceof CSSTransition &&
          a.transitionProperty === "stroke-dashoffset"
      );
    if (!forward) throw new Error("No forward transition");
    forward.pause();
    await forward.ready;
    forward.currentTime = 20;
    getComputedStyle(circle).strokeDashoffset;
    return original;
  }, duration);
  return page.evaluate(async (original) => {
    const circle = document.querySelector("[data-motion-test-ring]");
    if (!circle) throw new Error("No test ring");
    circle.setAttribute("stroke-dashoffset", original);
    getComputedStyle(circle).strokeDashoffset;
    const reversal = circle
      .getAnimations()
      .find(
        (a) =>
          a instanceof CSSTransition &&
          a.transitionProperty === "stroke-dashoffset"
      );
    if (!reversal) throw new Error("No reversal transition");
    // Keep genuine CSS shortening observable across separate browser calls.
    reversal.updatePlaybackRate(0);
    await reversal.ready;
    const duration = reversal.effect?.getTiming().duration;
    if (typeof duration !== "number") throw new Error("No reversal duration");
    return { duration, state: reversal.playState, rate: reversal.playbackRate };
  }, original);
}

test("live-motion validation accepts shortened reversals and rejects long transitions and entrances", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  const reversal = await reverseRing(page);
  expect(reversal.state).toBe("running");
  expect(reversal.rate).toBe(0);
  expect(reversal.duration).toBeGreaterThan(0);
  expect(reversal.duration).toBeLessThan(150);
  expect(await unexpectedRunning(page)).toEqual([]);

  await page.evaluate(() => {
    const transition = document
      .querySelector("[data-motion-test-ring]")
      ?.getAnimations()
      .find((a) => a instanceof CSSTransition);
    if (!transition?.effect) throw new Error("No ring transition effect");
    transition.effect.updateTiming({ duration: 400 });
  });
  expect(await unexpectedRunning(page)).toContain("stroke-dashoffset");

  const longDeclared = await reverseRing(page, 400);
  expect(longDeclared.duration).toBeLessThan(150);
  expect(await unexpectedRunning(page)).toContain("stroke-dashoffset");

  await openOverview(page);
  await expect.poll(() => running(page), { timeout: 2000 }).toBe(0);
  await page.evaluate(async () => {
    const element = Array.from(
      document.querySelectorAll("main > * > *, main [data-stagger] > *")
    ).find((el) => /lift/.test(getComputedStyle(el).animationName));
    if (!(element instanceof HTMLElement))
      throw new Error("No entering element");
    const name = getComputedStyle(element).animationName;
    element.style.animationName = "none";
    getComputedStyle(element).animationName;
    element.style.animationName = name;
    getComputedStyle(element).animationName;
    const entrance = element
      .getAnimations()
      .find((a) => a instanceof CSSAnimation && /lift/.test(a.animationName));
    if (!entrance) throw new Error("No entrance animation");
    entrance.updatePlaybackRate(0);
    await entrance.ready;
  });
  expect(
    (await unexpectedRunning(page)).some((name) => /lift/.test(name))
  ).toBe(true);
});

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
  await page.clock.install();
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await recordAnimations(page);
  await page.setViewportSize({ width: 1280, height: 833 });
  await page.goto("/?window=dashboard&theme=dark");
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
  await expect
    .poll(
      async () => (await started(page)).filter((n) => /lift/.test(n)).length
    )
    .toBeGreaterThan(0);
  await expect
    .poll(() => unexpectedRunning(page), { timeout: 2000 })
    .toEqual([]);

  await page.evaluate(() => {
    (window as unknown as { __anims: string[] }).__anims.length = 0;
  });
  // Three ticks of fresh values re-render every card.
  await page.clock.runFor(3000);
  expect((await started(page)).filter((n) => /lift|fade|swap/.test(n))).toEqual(
    []
  );
  expect(await unexpectedRunning(page)).toEqual([]);
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
