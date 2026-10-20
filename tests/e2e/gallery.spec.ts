import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";

/**
 * The dev gallery renders every widget and app component on the mock
 * transport. These specs screenshot it in both themes and hold muted text
 * to WCAG AA contrast.
 *
 * Screenshots land in test-results/screens/ (not compared against a
 * baseline; they are for the visual review in verification.md).
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

async function openGallery(page: Page, theme: string) {
  await page.setViewportSize({ width: 1440, height: 1000 });
  await page.goto(`/?route=/dev/gallery&theme=${theme}&ticks=0`);
  await expect(
    page.getByRole("heading", { name: "Widgets", exact: true })
  ).toBeVisible();
  await expect(page.locator("html")).toHaveClass(
    theme === "dark" ? /dark/ : /^(?!.*dark)/
  );
}

for (const theme of THEMES) {
  test.describe(`gallery (${theme})`, () => {
    test("renders without console errors and screenshots", async ({
      page,
    }, info) => {
      const errors = trackConsoleErrors(page);
      await openGallery(page, theme);
      await page.screenshot({
        path: `test-results/screens/gallery-${theme}-${info.project.name}.png`,
        fullPage: true,
      });
      expect(errors).toEqual([]);
    });

    test("muted-foreground text meets 4.5:1", async ({ page }) => {
      await openGallery(page, theme);
      const muted = page.locator(".text-muted-foreground");
      expect(await muted.count()).toBeGreaterThan(0);
      // The token's computed colour, so nested text in another colour (a
      // link inside a muted row) is not counted against it.
      const mutedColor = await page.evaluate(() => {
        const probe = document.createElement("span");
        probe.className = "text-muted-foreground";
        document.body.append(probe);
        const rgb = getComputedStyle(probe).color;
        probe.remove();
        const [r, g, b] = (rgb.match(/\d+/g) ?? []).map(Number);
        return `#${[r, g, b].map((n) => (n ?? 0).toString(16).padStart(2, "0")).join("")}`;
      });
      const results = await new AxeBuilder({ page })
        .include(".text-muted-foreground")
        .withRules(["color-contrast"])
        .analyze();
      const failures = results.violations.flatMap((v) =>
        v.nodes
          .filter((n) =>
            n.any.some(
              (c) =>
                (c.data as { fgColor?: string } | null)?.fgColor === mutedColor
            )
          )
          .map((n) => `${n.target.join(" ")}: ${n.failureSummary}`)
      );
      expect(failures).toEqual([]);
    });
  });
}

// Every window entry route. Dashboard pages other than Overview are phase 5
// placeholders and get their screenshots with the screens.
const ROUTES = [
  { name: "popover", url: "/?window=popover", ready: "CPU" },
  { name: "dashboard", url: "/?window=dashboard", ready: "CPU" },
  { name: "onboarding", url: "/?window=onboarding", ready: "Set up Kelvo" },
];

for (const theme of THEMES) {
  for (const route of ROUTES) {
    test(`${route.name} (${theme}) renders`, async ({ page }, info) => {
      const errors = trackConsoleErrors(page);
      await page.setViewportSize(
        route.name === "popover"
          ? { width: 360, height: 760 }
          : { width: 1280, height: 860 }
      );
      await page.goto(`${route.url}&theme=${theme}&ticks=0`);
      await expect(page.getByText(route.ready).first()).toBeVisible();
      await page.screenshot({
        path: `test-results/screens/${route.name}-${theme}-${info.project.name}.png`,
      });
      expect(errors).toEqual([]);
    });
  }
}
