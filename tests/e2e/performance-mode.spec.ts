import { expect, test } from "@playwright/test";

/**
 * Performance mode (D-088) end to end on the mock transport: the Settings
 * switch, what it overrides, the markers and their explainer, and Low Power
 * Mode turning it on without a settings write. Frame pacing and the reason's
 * resolution are covered by the mock and Rust tests; the mock does not keep
 * settings across a reload, so persistence is Rust's settings round trip.
 */

const settingsUrl = (scenario?: string) =>
  `/?window=dashboard&route=/dashboard/settings&theme=dark${scenario ? `&scenario=${scenario}` : ""}`;

test("the Settings switch turns the mode on and off", async ({ page }) => {
  await page.goto(settingsUrl());
  const toggle = page.getByRole("switch", { name: "Performance mode" });
  const battery = page.getByRole("switch", { name: "Slow down on battery" });
  const marker = page.getByRole("button", {
    name: "Performance mode, open Performance settings",
  });
  await expect(toggle).not.toBeChecked();
  await expect(page.getByText("Animations off")).toBeVisible();
  await expect(marker).toHaveCount(0);
  await expect(page.locator("html")).not.toHaveAttribute("data-performance");

  await toggle.click();
  await expect(page.locator("html")).toHaveAttribute("data-performance", "");
  await expect(marker).toBeVisible();
  await expect(battery).toBeDisabled();
  await expect(battery).toBeChecked();
  await expect(page.getByText("Set by Performance mode")).toBeVisible();

  await toggle.click();
  await expect(page.locator("html")).not.toHaveAttribute("data-performance");
  await expect(marker).toHaveCount(0);
  await expect(battery).toBeEnabled();
  await expect(battery).not.toBeChecked();
});

test("Low Power Mode turns it on and the sidebar marker says why", async ({
  page,
}) => {
  await page.goto(settingsUrl("low-power-mode"));
  await expect(page.locator("html")).toHaveAttribute("data-performance", "");
  const toggle = page.getByRole("switch", { name: "Performance mode" });
  await expect(toggle).toBeChecked();
  await expect(toggle).toBeDisabled();
  await expect(page.getByText("On while Low Power Mode is on")).toBeVisible();

  const marker = page.getByRole("button", {
    name: "Performance mode · Low Power, open Performance settings",
  });
  await marker.hover();
  await expect(
    page.getByText(/turned on because macOS Low Power Mode is on/).last()
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Performance settings", exact: true })
  ).toBeVisible();
});

test("the popover pill reads 2s · perf and explains itself on focus", async ({
  page,
}) => {
  await page.setViewportSize({ width: 360, height: 680 });
  await page.goto("/?window=popover&theme=dark&scenario=low-power-mode");
  const pill = page.getByRole("button", {
    name: "2s · perf, open Performance settings",
  });
  await expect(pill).toBeVisible();
  await pill.focus();
  await expect(
    page.getByRole("button", { name: "Performance settings", exact: true })
  ).toBeVisible();
});
