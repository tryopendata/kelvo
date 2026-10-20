import { expect, test } from "@playwright/test";

for (const theme of ["light", "dark"]) {
  test(`saved-host SSH verification guidance (${theme})`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(
      `/?route=/dashboard/hosts&scenario=multi-host&theme=${theme}&ticks=0`
    );
    await page.getByRole("button", { name: "Settings", exact: true }).click();
    await page
      .getByRole("button", { name: "Show SSH setup guide", exact: true })
      .click();
    const verification = page.getByText(/BatchMode=no/).first();
    await expect(verification).toContainText("StrictHostKeyChecking=ask");
    await expect(page.getByText(/BatchMode=yes/).first()).toContainText(
      "StrictHostKeyChecking=yes"
    );
    await expect(
      page.getByText(/for key in \/etc\/ssh\/ssh_host_/)
    ).toHaveCount(1);
    await verification.scrollIntoViewIfNeeded();
    await page.screenshot({
      path: `test-results/screens/ssh-saved-verification-${theme}-${info.project.name}.png`,
      animations: "disabled",
    });
  });

  test(`SSH authentication recovery (${theme})`, async ({ page }, info) => {
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(
      `/?route=/dashboard/hosts&scenario=ssh-auth&theme=${theme}&ticks=0`
    );
    await page.getByRole("button", { name: "Add host", exact: true }).click();
    await page
      .getByLabel("LAN SSH alias", { exact: true })
      .fill("user@office.local");
    await page
      .getByRole("button", { name: "Check connection", exact: true })
      .click();
    await expect(page.getByRole("alert")).toContainText(
      "unlock your key in your SSH agent"
    );
    await page.getByText("Technical details", { exact: true }).click();
    await expect(
      page.getByText("Permission denied (publickey).", { exact: true })
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/ssh-auth-${theme}-${info.project.name}.png`,
      animations: "disabled",
    });
    await page
      .getByLabel("LAN SSH alias", { exact: true })
      .fill("other@office.local");
    await expect(page.getByRole("alert")).toHaveCount(0);
    await expect(
      page.getByRole("button", { name: "Check connection", exact: true })
    ).toBeEnabled();
  });

  test(`SSH setup guidance without a config (${theme})`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(
      `/?route=/dashboard/hosts&scenario=ssh-no-config&theme=${theme}&ticks=0`
    );
    await page.getByRole("button", { name: "Add host", exact: true }).click();
    await expect(page.getByText(/No ~\/\.ssh\/config found/)).toBeVisible();
    await expect(
      page.getByRole("button", { name: "Hide SSH setup guide" })
    ).toHaveAttribute("aria-expanded", "true");
    await page
      .getByLabel("LAN SSH alias", { exact: true })
      .fill("user@office.local");
    await page.getByText(/Host studio/).scrollIntoViewIfNeeded();
    await page.screenshot({
      path: `test-results/screens/ssh-guide-${theme}-${info.project.name}.png`,
      animations: "disabled",
    });
    await page
      .getByRole("button", { name: "Check connection", exact: true })
      .click();
    await expect(
      page.getByRole("button", { name: "Connect", exact: true })
    ).toBeVisible();
    await page.getByRole("button", { name: "Cancel", exact: true }).click();
    await page.goto(`/?route=/dashboard/settings&theme=${theme}&ticks=0`);
    await expect(
      page.getByRole("heading", { name: "Settings", exact: true })
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/ssh-neighbor-settings-${theme}-${info.project.name}.png`,
      animations: "disabled",
    });
    await page.goto(`/?route=/dev/gallery&theme=${theme}&ticks=0`);
    await expect(
      page.getByRole("heading", { name: "Gallery", exact: true })
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/ssh-neighbor-gallery-${theme}-${info.project.name}.png`,
      animations: "disabled",
    });
  });
}

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
    await page.getByRole("button", { name: "Add host", exact: true }).click();
    await expect(page.getByRole("dialog")).toBeVisible();
    await page.screenshot({
      animations: "disabled",
      path: `test-results/screens/add-host-${theme}-${info.project.name}.png`,
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
  await page.getByRole("button", { name: "Add host", exact: true }).click();
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
  await page.getByRole("button", { name: "Add host", exact: true }).click();
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
  await page.getByRole("button", { name: "Remove host…", exact: true }).click();
  await expect(
    page.getByLabel("Delete mirrored history from this Mac")
  ).not.toBeChecked();
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await expect(page.getByLabel("LAN SSH alias", { exact: true })).toHaveValue(
    "office.local"
  );
  await page.getByRole("button", { name: "Remove host…", exact: true }).click();
  await page.getByRole("button", { name: "Remove host", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(
    page.getByRole("heading", { name: "Work Mac", exact: true })
  ).toHaveCount(0);
  await expect(
    page.getByText(/Add a Linux server or another Mac over SSH/)
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

for (const theme of ["light", "dark"]) {
  test(`Linux host setup and agent states (${theme})`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 1280, height: 833 });
    await page.goto(
      `/?route=/dashboard/hosts&scenario=linux-host,linux-agent-update&theme=${theme}&ticks=0`
    );
    await expect(
      page.getByRole("button", { name: "Update agent", exact: true })
    ).toBeVisible();
    await page.screenshot({
      animations: "disabled",
      path: `test-results/screens/hosts-linux-update-${theme}-${info.project.name}.png`,
    });
    await page.getByRole("button", { name: "Add host", exact: true }).click();
    await page
      .getByLabel("Tailscale SSH alias", { exact: true })
      .fill("linux-user");
    await page
      .getByRole("button", { name: "Check connection", exact: true })
      .click();
    await expect(
      page.getByText("sudo loginctl enable-linger riley", { exact: true })
    ).toBeVisible();
    await page.screenshot({
      animations: "disabled",
      path: `test-results/screens/add-host-linux-user-${theme}-${info.project.name}.png`,
    });
    await page
      .getByRole("button", { name: "Install and connect", exact: true })
      .click();
    await expect(
      page.getByRole("heading", { name: "build-box", exact: true })
    ).toBeVisible();
  });
}
