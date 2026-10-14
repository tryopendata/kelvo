import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

for (const theme of ["dark", "light"] as const) {
  for (const width of [360, 1280]) {
    test(`Disk I/O ${theme} ${width}px keeps attribution visible and accessible`, async ({
      page,
    }, info) => {
      await page.setViewportSize({ width, height: 900 });
      await page.goto(
        `/?window=dashboard&route=/dashboard/disk&theme=${theme}&ticks=0&scenario=disk-io-comparison`
      );
      await expect(
        page.getByRole("img", { name: /^Average request time:/ })
      ).toBeVisible();
      const plot = page.getByRole("img", { name: /^Disk I\/O:/ });
      expect(
        await plot.locator("span").evaluateAll((bars) =>
          bars.some((bar) => {
            const box = bar.getBoundingClientRect();
            return box.width >= 2 && box.height > 0;
          })
        )
      ).toBe(true);
      const table = page.getByRole("table", { name: "Disk processes · Live" });
      await expect(table.getByRole("row").nth(5)).toBeVisible();
      if (width === 1280) {
        const fifth = await table.getByRole("row").nth(5).boundingBox();
        expect((fifth?.y ?? 900) + (fifth?.height ?? 1)).toBeLessThanOrEqual(
          900
        );
      }
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= window.innerWidth
        )
      ).toBe(true);
      expect(
        await page
          .locator("main")
          .evaluate((el) => el.scrollWidth <= el.clientWidth)
      ).toBe(true);
      await page.screenshot({
        path: `test-results/screens/disk-io-${theme}-${width}-${info.project.name}.png`,
        fullPage: true,
      });
      if (width === 360) {
        await table.scrollIntoViewIfNeeded();
        await page.screenshot({
          path: `test-results/screens/disk-io-processes-${theme}-${width}-${info.project.name}.png`,
        });
      }
      const axe = await new AxeBuilder({ page })
        .include("main")
        .withTags(["wcag2a", "wcag2aa", "wcag21aa"])
        .analyze();
      expect(axe.violations).toEqual([]);
    });
  }
}

test("Disk compares workload with request time and preserves brushed process identity through Back", async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(
    "/?window=dashboard&route=/dashboard/disk&ticks=0&scenario=disk-io-comparison"
  );
  await expect(page.getByTestId("disk-response-current")).toContainText(
    "0.15 ms"
  );
  await page.getByRole("combobox", { name: "Disk device" }).click();
  await page.getByRole("option", { name: /External drive/ }).click();
  await expect(page.getByTestId("disk-response-current")).toContainText(
    "42 ms"
  );
  await page.getByRole("radio", { name: "Operations/s", exact: true }).click();
  await expect(
    page.getByRole("img", { name: /^Operations\/s:/ })
  ).toBeVisible();
  const brush = page
    .getByRole("slider", { name: "Select a time range" })
    .nth(1);
  await brush.focus();
  await page.keyboard.press("ArrowLeft");
  await expect(page.getByRole("tooltip")).toContainText("Read request");
  await expect(page.getByRole("tooltip")).toContainText("80 ops/s");
  await page.keyboard.press("Enter");
  const historical = page.getByRole("table", {
    name: "Disk processes · selected 10 s",
  });
  await expect(historical.getByText("6120 · Exited")).toBeVisible();
  await historical
    .getByRole("link", { name: "Inspect clang, PID 6120" })
    .click();
  await expect(
    page.getByRole("heading", { name: "clang", exact: true })
  ).toBeVisible();
  await expect(page.getByText(/Exited · Requested bytes/)).toBeVisible();
  await page.getByRole("link", { name: "Back to Disk" }).click();
  await expect(
    page.getByRole("combobox", { name: "Disk device" })
  ).toContainText("External drive");
  await expect(
    page.getByRole("table", { name: "Disk processes · selected 10 s" })
  ).toBeVisible();
  await page.getByRole("button", { name: "Live", exact: true }).click();
  await expect(
    page.getByRole("table", { name: "Disk processes · Live" })
  ).toBeVisible();
});

for (const [scenario, label] of [
  ["disk-io-no-ops", "No completed operations"],
  ["disk-io-unavailable", "Unavailable"],
]) {
  test(`Disk ${scenario} retains its missing-data meaning`, async ({
    page,
  }) => {
    await page.goto(
      `/?window=dashboard&route=/dashboard/disk&ticks=0&scenario=${scenario}`
    );
    await expect(page.getByTestId("disk-response-current")).toContainText(
      label ?? ""
    );
    await expect(page.getByTestId("disk-response-current")).not.toContainText(
      "0 ms"
    );
  });
}

test("Disk process details never use a different incarnation of the same PID", async ({
  page,
}) => {
  const route =
    "/dashboard/processes/6120/1?from=1700000000000&to=1700000010000";
  await page.goto(
    `/?window=dashboard&route=${encodeURIComponent(route)}&ticks=0`
  );
  await expect(
    page.getByRole("heading", { name: "Process unavailable" })
  ).toBeVisible();
  await expect(page.getByText(/A reused PID does not identify/)).toBeVisible();
});

test("a process checkbox at the viewport edge stays under the pointer during focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 900 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.clock.setFixedTime(new Date("2026-10-13T17:30:05Z"));
  await page.goto("/?window=dashboard&route=/dashboard/disk&ticks=0");
  const checkbox = page.getByRole("checkbox", {
    name: "Select Xcode (2214)",
    exact: true,
  });
  await checkbox.evaluate((element) => {
    const main = element.closest("main");
    if (!main) throw new Error("Process table must be inside the scrollport");
    // The checkbox is visible, but its taller row extends below the viewport.
    // WebKit focuses that row on pointer-down; passive focus must not scroll it.
    main.scrollTop += element.getBoundingClientRect().bottom - innerHeight + 1;
  });
  const bottom = await checkbox.evaluate(
    (element) => element.getBoundingClientRect().bottom
  );
  expect(bottom).toBeGreaterThan(898);
  expect(bottom).toBeLessThanOrEqual(900);
  await checkbox.click();
  await expect(checkbox).toBeChecked();
});

for (const width of [360, 1280]) {
  test(`Disk process selection keeps hidden targets and clears on a new interval at ${width}px`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height: 900 });
    // Keep the newest closed bucket inside the mock recording while the UI is exercised.
    await page.clock.setFixedTime(new Date("2026-10-13T17:30:05Z"));
    await page.goto("/?window=dashboard&route=/dashboard/disk&ticks=0");
    await page
      .getByRole("checkbox", { name: "Select Xcode (2214)", exact: true })
      .click();
    await expect(
      page.getByRole("checkbox", { name: "Select Xcode (2214)", exact: true })
    ).toBeChecked();
    await page
      .getByRole("checkbox", { name: "Select Figma (1712)", exact: true })
      .click();
    await expect(
      page.getByRole("checkbox", { name: "Select Figma (1712)", exact: true })
    ).toBeChecked();
    await page.getByRole("searchbox").fill("node");
    await expect(page.getByText("2 selected · 2 hidden")).toBeVisible();
    expect(
      await page
        .locator("main")
        .evaluate((el) => el.scrollWidth <= el.clientWidth)
    ).toBe(true);
    await page
      .getByRole("button", { name: "Selected only", exact: true })
      .click();
    await expect(
      page.getByRole("table", { name: "Selected processes" }).getByRole("row")
    ).toHaveCount(3);
    await page
      .getByRole("button", { name: "Force Quit 2…", exact: true })
      .click();
    const dialog = page.getByRole("alertdialog");
    await expect(dialog).toContainText("Xcode");
    await expect(dialog).toContainText("Figma");
    await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
    await page
      .getByRole("slider", { name: "Select a time range" })
      .first()
      .focus();
    await page.keyboard.press("Enter");
    await expect(
      page.getByRole("table", { name: /^Disk processes · selected/ })
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Force Quit 2…", exact: true })
    ).toHaveCount(0);
    await page.getByRole("searchbox").fill("");
    await expect(
      page.getByRole("checkbox", { name: "Select clang (6120)", exact: true })
    ).toBeDisabled();
  });
}
