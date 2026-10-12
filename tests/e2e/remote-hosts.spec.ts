import { expect, test } from "@playwright/test";

for (const theme of ["light", "dark"]) {
  test(`remote host selection and setup (${theme})`, async ({ page }, info) => {
    const errors: string[] = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(
      `/?route=/dashboard/hosts&scenario=multi-host&theme=${theme}&ticks=0`
    );
    await expect(
      page.getByRole("heading", { name: "Studio Mac", exact: true })
    ).toBeVisible();
    await page.screenshot({
      animations: "disabled",
      path: `test-results/screens/hosts-${theme}-${info.project.name}.png`,
    });
    await page.getByRole("button", { name: "Add Mac", exact: true }).click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await page.screenshot({
      animations: "disabled",
      path: `test-results/screens/add-mac-${theme}-${info.project.name}.png`,
    });
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.getByRole("combobox", { name: "Selected machine" }).click();
    await page.getByRole("option", { name: "Studio Mac", exact: true }).click();
    await page.getByRole("link", { name: "Overview", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Overview", exact: true })
    ).toBeVisible();
    await expect(page.getByRole("link", { name: /^Battery/ })).toHaveCount(0);
    await page.screenshot({
      animations: "disabled",
      path: `test-results/screens/remote-overview-${theme}-${info.project.name}.png`,
    });
    await page.getByRole("link", { name: "Processes", exact: true }).click();
    await expect(
      page.getByRole("heading", { name: "Processes", exact: true })
    ).toBeVisible();
    await expect(page.getByRole("button", { name: /Quit/ })).toHaveCount(0);
    await page.getByRole("link", { name: "Settings", exact: true }).click();
    await expect(
      page.getByRole("combobox", { name: "Selected machine" })
    ).toContainText("Studio Mac");
    await expect(
      page.getByRole("switch", { name: "Allow remote monitoring", exact: true })
    ).toBeVisible();
    expect(errors).toEqual([]);
  });
}

test("SSH host trust requires fingerprint confirmation", async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 1280, height: 833 });
  await page.goto(
    "/?route=/dashboard/hosts&scenario=ssh-untrusted&theme=dark&ticks=0"
  );
  await page.getByRole("button", { name: "Add Mac", exact: true }).click();
  await page.getByLabel("LAN SSH alias", { exact: true }).fill("office.local");
  await page
    .getByRole("button", { name: "Check connection", exact: true })
    .click();
  await page
    .getByRole("button", { name: "Verify LAN host", exact: true })
    .click();
  await expect(
    page.getByText(/SHA256:exampleMockHostFingerprint/)
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Connect", exact: true })
  ).toHaveCount(0);
  await page.screenshot({
    path: `test-results/screens/ssh-trust-${info.project.name}.png`,
    animations: "disabled",
  });
  await page
    .getByRole("button", { name: "Trust this host", exact: true })
    .click();
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "office.local", exact: true })
  ).toBeVisible();
});

test("a saved Mac can be renamed, disabled, selected, and removed without losing local access", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/?route=/dashboard/hosts&theme=dark&ticks=0");
  await page.getByRole("button", { name: "Add Mac", exact: true }).click();
  await page.getByLabel("Name", { exact: true }).fill("Office Mac");
  await page.getByLabel("LAN SSH alias", { exact: true }).fill("office.local");
  await page
    .getByRole("button", { name: "Check connection", exact: true })
    .click();
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Office Mac", exact: true })
  ).toBeVisible();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByLabel("Name", { exact: true }).fill("Work Mac");
  await page
    .getByRole("switch", { name: "Connect automatically", exact: true })
    .click();
  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Work Mac", exact: true })
  ).toBeVisible();
  await expect(
    page.getByText("Connection disabled", { exact: true })
  ).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Office Mac", exact: true })
  ).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: "Retry", exact: true })
  ).toHaveCount(0);
  await page.getByRole("combobox", { name: "Selected machine" }).click();
  await page.getByRole("option", { name: "Work Mac", exact: true }).click();
  await page.getByRole("link", { name: "Overview", exact: true }).click();
  await expect(
    page.getByRole("combobox", { name: "Selected machine" })
  ).toContainText("Work Mac");
  await expect(page.getByRole("link", { name: /^Battery/ })).toHaveCount(0);
  await page.getByRole("link", { name: "Hosts", exact: true }).click();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByLabel("Name", { exact: true })).toHaveValue(
    "Work Mac"
  );
  await expect(
    page.getByRole("switch", { name: "Connect automatically", exact: true })
  ).not.toBeChecked();
  await page.getByRole("button", { name: "Remove Mac…", exact: true }).click();
  await expect(
    page.getByLabel("Delete mirrored history from this Mac")
  ).not.toBeChecked();
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(page.getByLabel("LAN SSH alias", { exact: true })).toHaveValue(
    "office.local"
  );
  await page.getByRole("button", { name: "Remove Mac…", exact: true }).click();
  await page.getByRole("button", { name: "Remove Mac", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: "Work Mac", exact: true })
  ).toHaveCount(0);
  await expect(
    page.getByText(/Add a Mac to view its live metrics/)
  ).toBeVisible();
  await page.getByRole("button", { name: "View metrics", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
  await expect(
    page.getByRole("combobox", { name: "Selected machine" })
  ).toContainText("This Mac");
  await expect(page.getByRole("link", { name: /^Battery/ })).toBeVisible();
  expect(errors).toEqual([]);
});
