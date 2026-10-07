import { expect, test } from "@playwright/test";

/**
 * The backgrounded engine ticks at 2 s (D-094). A window that opens must see
 * the visible 1 s tick from its first status: if a 2 s status reached it, the
 * popover's 60 s charts would stretch to 120 s for a frame and re-key.
 *
 * The hidden-resume scenario hides the window on the mock's eleventh tick,
 * samples half an hour of 2 s rows and shows it again.
 */
test("the popover reopens on the 1 s tick with its 60 s charts", async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 760 });
  // Every CPU chart name the page ever shows, from the first render on.
  await page.addInitScript(() => {
    const seen: string[] = [];
    (window as unknown as { __cpuLabels: string[] }).__cpuLabels = seen;
    const record = () => {
      for (const el of document.querySelectorAll("[aria-label^='CPU, last']")) {
        const label = el.getAttribute("aria-label") ?? "";
        if (seen[seen.length - 1] !== label) seen.push(label);
      }
    };
    new MutationObserver(record).observe(document, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ["aria-label"],
    });
  });
  await page.goto("/?window=popover&theme=dark&scenario=hidden-resume");
  const resumedAt = () =>
    page.evaluate(
      () => performance.getEntriesByName("kelvo:resume")[0]?.startTime ?? null
    );
  await expect.poll(resumedAt, { timeout: 20_000 }).not.toBeNull();
  const labels = () =>
    page.evaluate(
      () => (window as unknown as { __cpuLabels: string[] }).__cpuLabels
    );
  const atResume = (await labels()).length;

  // Fresh numbers: the CPU value moves on after the resume.
  await expect
    .poll(async () => (await labels()).length, { timeout: 5000 })
    .toBeGreaterThan(atResume + 1);
  const all = await labels();
  expect(all.length).toBeGreaterThan(0);
  expect(all.filter((l) => !l.startsWith("CPU, last 60 seconds,"))).toEqual([]);
});
