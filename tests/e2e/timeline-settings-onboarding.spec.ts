import { expect, type Page, test } from "@playwright/test";

/**
 * Timeline, Settings and onboarding on the
 * mock transport, in both themes. Screenshots land in test-results/screens/
 * for the visual review (verification.md); they are not compared against a
 * baseline.
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

for (const theme of THEMES) {
  test.describe(`timeline (${theme})`, () => {
    test("24h lanes with the overnight sleep band", async ({ page }, info) => {
      const errors = trackConsoleErrors(page);
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto(
        `/?window=dashboard&route=/dashboard/timeline&theme=${theme}&ticks=0`
      );
      await expect(
        page.getByRole("heading", { name: "Timeline" })
      ).toBeVisible();
      await expect(page.locator("[data-lane-plot]")).toHaveCount(6);
      await expect(page.getByText("Asleep 5h 15m · no samples")).toBeVisible();
      await expect(page.getByText(/Sleep$/).first()).toBeVisible();
      await page.screenshot({
        path: shot("timeline-24h", theme, info.project.name),
        fullPage: true,
      });
      expect(errors).toEqual([]);
    });

    test("1h with a sleep gap and the crosshair", async ({ page }, info) => {
      const errors = trackConsoleErrors(page);
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto(
        `/?window=dashboard&route=/dashboard/timeline&scenario=sleep-gap&theme=${theme}&ticks=0`
      );
      await page.getByRole("radio", { name: "1h", exact: true }).click();
      await expect(
        page.getByText(/^Asleep \d\d:\d\d–\d\d:\d\d/).first()
      ).toBeVisible();

      const cursor = page.getByRole("slider", { name: "Timeline cursor" });
      const box = await cursor.boundingBox();
      if (!box) throw new Error("no cursor overlay");
      await page.mouse.move(box.x + box.width * 0.85, box.y + box.height * 0.4);
      await expect(page.getByText("Top processes then")).toBeVisible();
      await expect(page.getByText("Xcode").first()).toBeVisible();
      await page.screenshot({
        path: shot("timeline-1h-sleep-gap", theme, info.project.name),
        fullPage: true,
      });
      expect(errors).toEqual([]);
    });
  });

  test.describe(`settings (${theme})`, () => {
    test("renders and writes through update_settings", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await page.setViewportSize({ width: 1280, height: 860 });
      await page.goto(
        `/?window=dashboard&route=/dashboard/settings&theme=${theme}&ticks=0`
      );
      await expect(
        page.getByRole("heading", { name: "Modules" })
      ).toBeVisible();
      await expect(page.getByText("148 MB")).toBeVisible();
      await page.screenshot({
        path: shot("settings", theme, info.project.name),
        fullPage: true,
      });
      // Own-item modes in the CPU separate item select (D-080, D-102).
      await page.getByRole("combobox", { name: "CPU separate item" }).click();
      await expect(
        page.getByRole("option", { name: "Per-core graph" })
      ).toBeVisible();
      // Past the list's open animation, so the screenshot shows it settled.
      await page
        .getByRole("listbox")
        .evaluate((el) =>
          Promise.all(
            el.getAnimations({ subtree: true }).map((a) => a.finished)
          )
        );
      await page.screenshot({
        path: shot("settings-cpu-menu-bar", theme, info.project.name),
        fullPage: true,
      });
      await page.keyboard.press("Escape");

      const disk = page.getByRole("switch", { name: "Disk enabled" });
      await expect(disk).toBeChecked();
      await disk.click();
      await expect(disk).not.toBeChecked();
      await expect(
        page.getByRole("combobox", { name: "Disk separate item" })
      ).toBeDisabled();
      await expect(
        page.getByRole("switch", { name: "Show disk used in the menu bar" })
      ).toBeDisabled();

      await page.getByRole("button", { name: "Clear", exact: true }).click();
      await page.getByRole("button", { name: "Clear history" }).click();
      await expect(page.getByRole("alertdialog")).toBeHidden();
      await expect(page.getByText("0 MB", { exact: true })).toBeVisible();
      expect(errors).toEqual([]);
    });
  });

  test.describe(`onboarding (${theme})`, () => {
    test("step 1, Continue, step 2", async ({ page }, info) => {
      const errors = trackConsoleErrors(page);
      await page.setViewportSize({ width: 820, height: 566 });
      await page.goto(`/?window=onboarding&theme=${theme}&ticks=0`);
      await expect(
        page.getByRole("heading", { name: "Set up Kelvo" })
      ).toBeVisible();
      await expect(
        page.getByText("M4 Pro detected · all sensors mapped")
      ).toBeVisible();
      await expect(
        page.getByRole("radio", { name: /Combined/ })
      ).toHaveAttribute("aria-checked", "true");
      // v1.1: the third card, inside the fixed window with Continue still on screen.
      await expect(
        page.getByRole("radio", { name: /Graph per module/ })
      ).toBeInViewport();
      await expect(
        page.getByRole("button", { name: "Continue" })
      ).toBeInViewport();
      const last = await page
        .getByRole("radio", { name: /Values only/ })
        .boundingBox();
      const footer = await page.locator("footer").boundingBox();
      expect(last && footer && last.y + last.height <= footer.y).toBe(true);
      await page.screenshot({
        path: shot("onboarding-step1", theme, info.project.name),
      });

      await page.getByRole("button", { name: "Continue" }).click();
      await expect(
        page.getByRole("heading", { name: "Updates and privacy" })
      ).toBeVisible();
      // The size comes from the live series count (history-projection.ts).
      await expect(
        page.getByText(/takes about \d+ MB for 30 days/)
      ).toBeVisible();
      await page.screenshot({
        path: shot("onboarding-step2", theme, info.project.name),
      });
      expect(errors).toEqual([]);
    });
  });
}
