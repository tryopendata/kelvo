import { expect, test } from "@playwright/test";

test("selected remote Mac stays selected across local Settings and Hosts pages", async ({
  page,
}) => {
  await page.goto(
    "/?route=/dashboard/hosts&scenario=multi-host&theme=dark&ticks=0"
  );
  const switcher = page.getByRole("combobox", { name: "Selected machine" });
  await switcher.click();
  await page.getByRole("option", { name: "Studio Mac", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Overview", exact: true })
  ).toBeVisible();
  // Observe every text mutation, including a transient local-host flip.
  await switcher.evaluate((element) => {
    const state = window as typeof window & { hostFlipped?: boolean };
    state.hostFlipped = false;
    new MutationObserver(() => {
      if (element.textContent?.includes("This Mac")) state.hostFlipped = true;
    }).observe(element, {
      subtree: true,
      childList: true,
      characterData: true,
    });
  });
  for (const route of ["Settings", "Hosts", "Overview"]) {
    await page.getByRole("link", { name: route, exact: true }).click();
    await expect(
      page.getByRole("heading", { name: route, exact: true })
    ).toBeVisible();
    await expect(switcher).toContainText("Studio Mac");
  }
  expect(
    await page.evaluate(
      () => (window as typeof window & { hostFlipped?: boolean }).hostFlipped
    )
  ).toBe(false);
});
