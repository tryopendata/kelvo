import { expect, type Page, test } from "@playwright/test";

const NOW = new Date(2026, 9, 4, 22, 40);
const cursor = (page: Page) =>
  page.getByRole("slider", { name: "Timeline cursor" });
const header = (page: Page) =>
  page.getByRole("heading", { name: "Timeline", exact: true }).locator("..");
async function ready(page: Page, theme: string, scenario = "default") {
  await page.clock.setFixedTime(NOW);
  await page.setViewportSize({ width: 1360, height: 1000 });
  await page.goto(
    `/?window=dashboard&route=/dashboard/timeline&theme=${theme}&scenario=${scenario}&ticks=0`
  );
  await expect(cursor(page)).toHaveAttribute("aria-busy", "false");
  await expect(page.getByText(/Average · peak/).first()).toBeVisible();
}
async function drag(page: Page, release = true) {
  const box = await cursor(page).boundingBox();
  if (!box) throw new Error("Timeline plot unavailable");
  await page.mouse.move(box.x + box.width * 0.3, box.y + 40);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.7, box.y + 60, { steps: 8 });
  if (release) await page.mouse.up();
}
async function settled(page: Page) {
  await expect(cursor(page)).toHaveAttribute("aria-busy", "false");
  await expect(page.getByText(/Loading recorded detail/)).toHaveCount(0);
}

for (const theme of ["light", "dark"]) {
  test(`${theme}: a released selection propagates across tabs and Back restores Live`, async ({
    page,
  }, info) => {
    await ready(page, theme);
    await drag(page, false);
    await expect(header(page)).toContainText("Live ·");
    await expect(
      page.getByRole("button", { name: "Back", exact: true })
    ).toBeDisabled();
    await page.mouse.up();
    await expect(header(page)).toContainText("Recorded ·");
    await settled(page);
    const dates = (await header(page).innerText()).match(
      /\d{4}-\d\d-\d\d \d\d:\d\d(?::\d\d(?:\.\d+)?)? to \d{4}-\d\d-\d\d \d\d:\d\d(?::\d\d(?:\.\d+)?)?/
    )?.[0];
    if (!dates) throw new Error("Selected dates not visible");
    await page.screenshot({
      path: `test-results/screens/timeline-selection-${theme}-${info.project.name}.png`,
    });
    await page.getByRole("link", { name: /^CPU / }).click();
    await expect(page.getByText(dates, { exact: false }).first()).toBeVisible();
    await page.getByRole("link", { name: "Timeline", exact: true }).click();
    await page.getByRole("button", { name: "Back", exact: true }).click();
    await expect(header(page)).toContainText("Live ·");
    await expect(
      page.getByRole("button", { name: "Back", exact: true })
    ).toBeDisabled();
  });

  test(`${theme}: Escape cancels a drag and Shift arrows commit a keyboard range`, async ({
    page,
  }) => {
    await ready(page, theme);
    await drag(page, false);
    await page.keyboard.press("Escape");
    await page.mouse.up();
    await expect(header(page)).toContainText("Live ·");
    await expect(
      page.getByRole("button", { name: "Back", exact: true })
    ).toBeDisabled();
    await cursor(page).focus();
    await page.keyboard.press("Shift+ArrowLeft");
    await page.keyboard.press("Enter");
    await expect(header(page)).toContainText("Recorded ·");
    await settled(page);
    await expect(cursor(page)).toBeFocused();
    await page.getByRole("button", { name: "Live", exact: true }).click();
    await expect(header(page)).toContainText("Live ·");
  });

  test(`${theme}: a fixed range reaches Battery and Containers history`, async ({
    page,
  }) => {
    await ready(page, theme, "docker-11");
    await drag(page);
    await expect(header(page)).toContainText("Recorded ·");
    await settled(page);
    const dates = (await header(page).innerText()).match(
      /\d{4}-\d\d-\d\d \d\d:\d\d(?::\d\d(?:\.\d+)?)? to \d{4}-\d\d-\d\d \d\d:\d\d(?::\d\d(?:\.\d+)?)?/
    )?.[0];
    if (!dates) throw new Error("Selected dates not visible");
    await page.getByRole("link", { name: /^Battery / }).click();
    await expect(
      page.getByRole("img", { name: "Battery charge, selected range" })
    ).toBeVisible();
    await expect(page.getByText(dates, { exact: false }).first()).toBeVisible();
    await page.getByRole("link", { name: "Containers", exact: true }).click();
    await expect(page.getByText(dates, { exact: false }).first()).toBeVisible();
    await expect(
      page.getByRole("table", { name: /^Containers · / })
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Next container range" })
    ).toBeVisible();
  });
}

for (const scenario of ["reduced-motion", "performance-mode"]) {
  test(`${scenario}: selection settles without a zoom preview`, async ({
    page,
  }) => {
    if (scenario === "reduced-motion")
      await page.emulateMedia({ reducedMotion: "reduce" });
    await ready(
      page,
      "dark",
      scenario === "performance-mode" ? scenario : "default"
    );
    await drag(page);
    await expect(header(page)).toContainText("Recorded ·");
    await expect(page.getByText(/preview at/)).toHaveCount(0);
    await expect(cursor(page)).toHaveAttribute("aria-busy", "false");
  });
}

test("pause explanation remains available through chart hover and keyboard focus", async ({
  page,
}) => {
  await ready(page, "dark", "paused");
  const note = page
    .getByRole("note", {
      name: "Recording was paused during this interval. No samples were collected.",
    })
    .first();
  await expect(note).toBeAttached();
  const box = await note.boundingBox();
  if (!box) throw new Error("Pause band unavailable");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await expect(page.getByRole("tooltip")).toContainText(
    "Recording was paused during this interval. No samples were collected."
  );
  await cursor(page).focus();
  await page.keyboard.press("End");
  await expect(page.getByRole("tooltip")).toContainText(
    "Recording was paused during this interval. No samples were collected."
  );
});

for (const theme of ["light", "dark"]) {
  test(`${theme}: previous interval comparison and recorded process details stay available after ranking`, async ({
    page,
  }, info) => {
    await ready(page, theme);
    await drag(page);
    await settled(page);
    await page
      .getByRole("button", { name: "Compare previous interval" })
      .click();
    await expect(page.getByText(/Previous:/).first()).toBeVisible();
    await expect(page.getByText("Compiler", { exact: true })).toBeVisible();
    await page
      .getByRole("button", { name: "Rank apps by Memory", exact: true })
      .click();
    await expect(
      page.getByRole("columnheader", { name: "Peak RAM" })
    ).toHaveAttribute("aria-sort", "descending");
    await page
      .getByRole("button", { name: "Show Compiler recorded processes" })
      .click();
    await expect(page.getByText("PID 4099", { exact: false })).toBeVisible();
    await page
      .getByRole("button", { name: "Load more recorded processes" })
      .click();
    await expect(page.getByText("PID 4124", { exact: false })).toBeVisible();
    await page
      .getByRole("button", { name: "Hide Compiler recorded processes" })
      .click();
    await page
      .getByRole("table", { name: "Recorded apps in this range" })
      .scrollIntoViewIfNeeded();
    await page.screenshot({
      path: `test-results/screens/timeline-comparison-${theme}-${info.project.name}.png`,
    });
  });
}
