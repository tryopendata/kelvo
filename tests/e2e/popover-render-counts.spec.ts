import { expect, test } from "@playwright/test";

test("mock frames commit once with real browser observers and no act batching", async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 900 });
  await page.goto("/tests/e2e/fixtures/popover-profile/index.html");
  await expect(
    page.getByRole("button", { name: "Collapse CPU" })
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: /Copy public IP/ })
  ).toBeVisible();
  // Let initial width observers and query notifications settle before measuring ticks.
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()))
      )
  );
  for (let tick = 0; tick < 3; tick++) {
    const result = await page.evaluate(async () => {
      window.popoverProfile.commits.length = 0;
      window.popoverProfile.messages.length = 0;
      window.popoverProfile.tick();
      await new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve()))
      );
      return {
        commits: window.popoverProfile.commits,
        messages: window.popoverProfile.messages,
      };
    });
    expect(result.messages.filter((kind) => kind === "frame")).toHaveLength(1);
    expect(result.commits).toHaveLength(1);
  }
});
