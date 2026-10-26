import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await page.goto("/tests/e2e/fixtures/tick-scroll/index.html");
  await page.locator('[data-scroll="svg"]').waitFor();
});

test("slides HTML and SVG to rest without forcing geometry reads", async ({
  page,
}) => {
  const result = await page.evaluate(() => {
    window.tickScroll.update(2000);
    return [...document.querySelectorAll("[data-scroll]")].map((element) => {
      const animation = element.getAnimations()[0];
      if (!animation) throw new Error("Expected tick animation");
      animation.pause();
      animation.currentTime = 75;
      const middle = new DOMMatrix(getComputedStyle(element).transform).m41;
      animation.finish();
      return {
        middle,
        end: getComputedStyle(element).transform,
        reads: window.tickScroll.reads,
      };
    });
  });
  for (const item of result) {
    expect(item.reads).toBe(0);
    expect(item.middle).toBeGreaterThan(0);
    expect(item.middle).toBeLessThan(10);
    expect(["none", "matrix(1, 0, 0, 1, 0, 0)"]).toContain(item.end);
  }
});

test("zero motion lands immediately and gaps cancel an in-flight slide", async ({
  page,
}) => {
  const result = await page.evaluate(() => {
    window.tickScroll.update(2000);
    for (const animation of document.getAnimations()) animation.pause();
    window.tickScroll.update(9000);
    const afterGap = document.getAnimations().length;
    document.documentElement.style.setProperty("--motion-tick", "0ms");
    window.tickScroll.update(10000);
    return {
      afterGap,
      afterDisabled: document.getAnimations().length,
      transforms: [...document.querySelectorAll("[data-scroll]")].map(
        (element) => getComputedStyle(element).transform
      ),
      reads: window.tickScroll.reads,
    };
  });
  expect(result.afterGap).toBe(0);
  expect(result.afterDisabled).toBe(0);
  expect(result.reads).toBe(0);
  for (const transform of result.transforms)
    expect(["none", "matrix(1, 0, 0, 1, 0, 0)"]).toContain(transform);
});

test("unmount cancels retained animations", async ({ page }) => {
  expect(
    await page.evaluate(() => {
      window.tickScroll.update(2000);
      const animations = document.getAnimations();
      for (const animation of animations) animation.pause();
      window.tickScroll.unmount();
      return animations.map((animation) => animation.playState);
    })
  ).toEqual(["idle", "idle"]);
});

test("live reduced-motion and Performance mode tokens prevent scrolling", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  expect(
    await page.evaluate(() => {
      window.tickScroll.update(2000);
      return document.getAnimations().length;
    })
  ).toBe(0);
  await page.emulateMedia({ reducedMotion: "no-preference" });
  expect(
    await page.evaluate(() => {
      document.documentElement.setAttribute("data-performance", "");
      window.tickScroll.update(3000);
      return document.getAnimations().length;
    })
  ).toBe(0);
});

test("area charts move an HTML layer while grid, markers and labels stay fixed", async ({
  page,
}, testInfo) => {
  await page.goto("/tests/e2e/fixtures/tick-scroll/index.html?chart=1");
  await expect(page.getByRole("img", { name: "CPU test chart" })).toBeVisible();
  const result = await page.evaluate(() => {
    const chart = document.querySelector("[data-chart]");
    if (!chart) throw new Error("Missing chart fixture");
    const image = chart.querySelector('svg[role="img"]');
    const grid = chart.querySelector('path[stroke="var(--color-grid)"]');
    const marker = chart.querySelector("[data-gap-edge]");
    if (!image || !grid || !marker) throw new Error("Missing plot layers");
    const gridLeft = grid.getBoundingClientRect().left;
    const markerLeft = marker.getBoundingClientRect().left;
    const imageLeft = image.getBoundingClientRect().left;
    window.tickScroll.update(2000);
    const animation = document.getAnimations()[0];
    if (!animation || !(animation.effect instanceof KeyframeEffect))
      throw new Error("Missing tick animation");
    const target = animation.effect.target;
    animation.pause();
    animation.currentTime = 0;
    return {
      htmlTarget: target instanceof HTMLElement,
      shift: image.getBoundingClientRect().left - imageLeft,
      gridShift: grid.getBoundingClientRect().left - gridLeft,
      markerShift: marker.getBoundingClientRect().left - markerLeft,
      clip: target?.parentElement
        ? getComputedStyle(target.parentElement).overflow
        : null,
    };
  });
  expect(result.htmlTarget).toBe(true);
  expect(result.shift).toBeCloseTo(50, 1);
  expect(result.gridShift).toBe(0);
  expect(result.markerShift).toBe(0);
  expect(result.clip).toBe("hidden");
  await expect(page.getByRole("img")).toHaveCount(1);
  await expect(page.getByText("100%", { exact: true })).toBeVisible();
  await expect(page.getByText("4s", { exact: true })).toBeVisible();
  await page
    .locator("[data-chart]")
    .screenshot({ path: testInfo.outputPath("chart-scroll-start.png") });
  await page.evaluate(() => {
    for (const animation of document.getAnimations()) animation.finish();
    // Compare paint order independently of compositor rasterization differences.
    document.documentElement.setAttribute("data-performance", "");
  });
  const settled = await page.locator("[data-chart]").screenshot();
  // Recreate the previous single-SVG paint order as a visual reference.
  await page.evaluate(() => {
    const image = document.querySelector('[data-chart] svg[role="img"]');
    const grid = document.querySelector("[data-chart] svg[aria-hidden]");
    const clip = image?.parentElement?.parentElement;
    if (!image || !grid || !clip) throw new Error("Missing plot layers");
    const series = image.querySelector("g");
    for (const path of [...grid.childNodes]) image.insertBefore(path, series);
    clip.replaceChildren(image);
  });
  expect(await page.locator("[data-chart]").screenshot()).toEqual(settled);
});

for (const mode of ["chart", "bars"]) {
  test(`${mode} scroll layers retain transforms only while motion is enabled`, async ({
    page,
  }) => {
    await page.goto(`/tests/e2e/fixtures/tick-scroll/index.html?${mode}=1`);
    await page.getByRole("img").waitFor();
    const transforms = () =>
      page.evaluate(() => {
        const chart = document.querySelector('[role="img"]');
        const layers =
          chart instanceof SVGElement
            ? [chart.parentElement]
            : [chart?.firstElementChild, chart?.lastElementChild];
        return layers.map((layer) =>
          layer ? getComputedStyle(layer).transform : "missing"
        );
      });
    for (const transform of await transforms())
      expect(transform).not.toBe("none");
    await page.evaluate(() =>
      document.documentElement.setAttribute("data-performance", "")
    );
    for (const transform of await transforms()) expect(transform).toBe("none");
    await page.evaluate(() =>
      document.documentElement.removeAttribute("data-performance")
    );
    await page.emulateMedia({ reducedMotion: "reduce" });
    for (const transform of await transforms()) expect(transform).toBe("none");
  });
}
