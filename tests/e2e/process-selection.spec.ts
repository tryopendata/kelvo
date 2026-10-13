import { expect, test } from "@playwright/test";

for (const theme of ["dark", "light"] as const) {
  test(`bulk selection, hidden review and confirmation (${theme})`, async ({
    page,
  }, info) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(
      `/?window=dashboard&route=/dashboard/processes&theme=${theme}&ticks=0`
    );
    const xcode = page.getByRole("cell", { name: "Xcode", exact: true });
    const figma = page.getByRole("cell", { name: "Figma", exact: true });
    await xcode.click();
    await figma.click({ modifiers: ["Meta"] });
    await expect(page.getByText("2 selected", { exact: true })).toBeVisible();
    await page.getByRole("searchbox").fill("node");
    await expect(page.getByText("2 selected · 2 hidden")).toBeVisible();
    await page
      .getByRole("button", { name: "Selected only", exact: true })
      .click();
    await expect(
      page.getByRole("table", { name: "Selected processes" }).getByRole("row")
    ).toHaveCount(3);
    await page.screenshot({
      path: `test-results/screens/process-selection-${theme}-${info.project.name}.png`,
    });
    await page
      .getByRole("button", { name: "Force Quit 2…", exact: true })
      .click();
    const dialog = page.getByRole("alertdialog", {
      name: "Force quit 2 processes?",
    });
    await expect(dialog).toContainText("Xcode");
    await expect(dialog).toContainText("Figma");
    await page.screenshot({
      path: `test-results/screens/process-batch-confirm-${theme}-${info.project.name}.png`,
    });
    await dialog
      .getByRole("button", { name: "Force Quit 2 processes", exact: true })
      .click();
    await expect(
      page.getByText("Batch results: 2 of 2 requests accepted")
    ).toBeVisible();
    await page.getByRole("link", { name: /^CPU/ }).click();
    await page.getByRole("link", { name: "Processes", exact: true }).click();
    await expect(page.getByText("2 selected", { exact: true })).toHaveCount(0);
  });
}

test("batch results fit above the footer of a full live table", async ({
  page,
}) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/?window=dashboard&route=/dashboard/processes&ticks=0");
  await page.getByRole("checkbox", { name: "Select Xcode (2214)" }).click();
  await page.getByRole("checkbox", { name: "Select Figma (1712)" }).click();
  await page.getByRole("button", { name: "Quit 2…", exact: true }).click();
  await page
    .getByRole("button", { name: "Quit 2 processes", exact: true })
    .click();
  const results = page.getByText("Batch results: 2 of 2 requests accepted");
  await results.click();
  const detail = page.locator("details[open]");
  await expect(detail).toContainText("Asked Xcode");
  const bounds = await detail.boundingBox();
  const footer = await page.locator("footer").last().boundingBox();
  expect(bounds).not.toBeNull();
  expect(footer).not.toBeNull();
  if (bounds && footer)
    expect(bounds.y + bounds.height).toBeLessThanOrEqual(footer.y + 1);
});

test("native selection modifiers, virtual keyboard navigation and gutter dragging", async ({
  page,
}) => {
  await page.goto("/?window=dashboard&route=/dashboard/processes&ticks=0");
  const table = page.getByRole("table", { name: "Processes", exact: true });
  const xcode = table.getByRole("row", { name: /Xcode/ });
  await xcode.click();
  await table
    .getByRole("row", { name: /Figma/ })
    .click({ modifiers: ["Shift"] });
  await expect(page.getByText("6 selected", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Clear", exact: true }).click();
  await xcode.focus();
  await page.keyboard.press("Meta+End");
  await expect(table.locator("tr[data-process-key]:focus")).toHaveCount(1);
  await page.keyboard.press("Space");
  await expect(page.getByText("1 selected", { exact: true })).toBeVisible();
  await page.keyboard.press("Meta+a");
  await expect(page.getByText("800 selected", { exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Home");
  await page.getByRole("button", { name: "Clear", exact: true }).click();
  const start = await xcode.locator("[data-selection-gutter]").boundingBox();
  const end = await table.getByRole("row", { name: /Figma/ }).boundingBox();
  if (!start || !end) throw new Error("Missing drag rows");
  await page.mouse.move(start.x + 2, start.y + start.height / 2);
  await page.mouse.down();
  await page.mouse.move(start.x + 2, end.y + end.height / 2, { steps: 8 });
  await page.mouse.up();
  await expect(page.getByText("6 selected", { exact: true })).toBeVisible();
  await xcode.click({ button: "right" });
  await expect(
    page.getByRole("menuitem", { name: "Quit 6 processes…", exact: true })
  ).toBeVisible();
});

for (const [route, tableName] of [
  ["cpu", "CPU"],
  ["gpu", "GPU"],
  ["memory", "Memory"],
  ["disk", "Disk"],
  ["power", "Energy"],
] as const) {
  test(`${route} selects an app as its main process and adds members`, async ({
    page,
  }, info) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(
      `/?window=dashboard&route=/dashboard/${route}&theme=dark&ticks=0`
    );
    if (route === "disk")
      await page.getByRole("button", { name: "By app", exact: true }).click();
    const table = page.getByRole("table", {
      name: `${tableName} by app`,
      exact: true,
    });
    const groupName = route === "gpu" ? "Safari" : "Xcode";
    const group = table.getByRole("checkbox", {
      name: `Select ${groupName}`,
      exact: true,
    });
    // An app selects as its main process; members are added one by one.
    await group.click();
    await expect(page.getByText("1 selected", { exact: true })).toBeVisible();
    await table
      .getByRole("button", { name: `Show ${groupName} processes` })
      .click();
    const member = /Select .+ \(\d+\)/;
    await expect(
      table.getByRole("checkbox", { name: member, checked: true })
    ).toHaveCount(1);
    await table
      .getByRole("checkbox", { name: member, checked: false })
      .locator(":scope:enabled")
      .first()
      .click();
    await expect(page.getByText("2 selected", { exact: true })).toBeVisible();
    await expect(group).toBeChecked();
    await page.screenshot({
      path: `test-results/screens/process-group-${route}-${info.project.name}.png`,
    });
    await page
      .getByRole("button", { name: /^Force Quit \d+…$/, exact: true })
      .click();
    await expect(page.getByRole("alertdialog")).toBeVisible();
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
  });
}
