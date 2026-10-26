import { expect, test } from "@playwright/test";

for (const theme of ["dark", "light"]) {
  for (const variant of ["", ",power-disabled", ",no-battery"]) {
    test(`compact ${theme} ${variant || "combined battery"} fits 360 x 680 with Containers`, async ({
      page,
    }, info) => {
      await page.setViewportSize({ width: 360, height: 680 });
      await page.goto(
        `/?window=popover&theme=${theme}&scenario=popover-compact,docker-11${variant}`
      );
      await expect(
        page.getByRole("button", { name: "Expand CPU" })
      ).toBeVisible();
      await expect(
        page.getByRole("button", { name: "Expand Containers" })
      ).toBeVisible();
      const viewport = page.locator("[data-radix-scroll-area-viewport]");
      await expect
        .poll(() =>
          viewport.evaluate((el) => el.scrollHeight - el.clientHeight)
        )
        .toBeLessThanOrEqual(1);
      const sizes = await page
        .locator("[data-popover-size]")
        .evaluateAll((regions) =>
          regions.map((region) => ({
            height: region.getBoundingClientRect().height,
            width: region.getBoundingClientRect().width,
          }))
        );
      console.log(`fit ${theme}${variant}: ${JSON.stringify(sizes)}`);
      await page.screenshot({
        path: `test-results/screens/compact-${theme}${variant.replaceAll(",", "-") || "-combined"}-${info.project.name}.png`,
      });
      if (variant.includes("power-disabled")) {
        await expect(
          page.getByRole("link", { name: "Battery", exact: true })
        ).toBeVisible();
      } else if (!variant.includes("no-battery")) {
        await expect(
          page.getByRole("link", { name: "Power & battery" })
        ).toBeVisible();
        await expect(
          page.getByRole("link", { name: "Battery", exact: true })
        ).toHaveCount(0);
      }
      await page.setViewportSize({ width: 360, height: 300 });
      await expect
        .poll(() =>
          viewport.evaluate((el) => el.scrollHeight - el.clientHeight)
        )
        .toBeGreaterThan(0);
      await expect(
        page.getByRole("button", { name: "Open dashboard" })
      ).toBeVisible();
    });
  }
}

test("mixed density keeps focus and exposes expanded details without opening a dashboard", async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 680 });
  await page.goto("/?window=popover&scenario=popover-compact,docker-11");
  await page.getByRole("button", { name: "Expand CPU" }).click();
  await expect(
    page.getByRole("button", { name: "Collapse CPU" })
  ).toBeFocused();
  await expect(page.getByText("User", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Expand GPU" })).toBeVisible();
  await page.getByRole("button", { name: "Expand Containers" }).click();
  await expect(
    page.getByRole("button", { name: "Collapse Containers" })
  ).toBeFocused();
  await page.getByRole("button", { name: "Collapse Containers" }).click();
  await expect(
    page.getByRole("button", { name: "Expand Containers" })
  ).toBeFocused();
  await page.getByRole("button", { name: "Popover options" }).click();
  await page.getByRole("menuitem", { name: "Collapse all" }).click();
  await expect(page.getByText("User", { exact: true })).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Expand Containers" })
  ).toBeVisible();
});

for (const host of ["default", "linux"]) {
  test(`Memory pressure/used and Swap are full width on ${host}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 1280, height: 1000 });
    await page.goto(
      `/?window=dashboard&route=/dashboard/memory&scenario=${host}`
    );
    const swap = page.getByRole("region", { name: /^Swap,/ });
    await expect(swap).toBeVisible();
    const pressure = page.getByRole("region", {
      name: host === "linux" ? "Used" : "Pressure",
    });
    const a = await pressure.boundingBox();
    const b = await swap.boundingBox();
    expect(a).not.toBeNull();
    expect(b).not.toBeNull();
    expect(b?.width).toBe(a?.width);
    expect(b?.y).toBeGreaterThan((a?.y ?? 0) + (a?.height ?? 0));
  });
}
