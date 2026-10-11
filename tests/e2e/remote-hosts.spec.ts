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
    ).toContainText("This Mac");
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
