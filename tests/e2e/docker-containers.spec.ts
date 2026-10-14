import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

const overview = "/?route=/dashboard/containers&scenario=docker-32&ticks=0";
const activity = "/?route=/dashboard/containers&scenario=docker-11&ticks=0";
const detail =
  "/?route=/dashboard/containers/mock-orbstack/container-0&scenario=docker-32&ticks=0";

test("monitoring is opt-in and unavailable engines remain visible", async ({
  page,
}) => {
  await page.goto("/?route=/dashboard/containers&ticks=0");
  await expect(
    page.getByRole("button", { name: "Enable monitoring" })
  ).toBeVisible();
  await page.getByRole("button", { name: "Monitoring…" }).click();
  await expect(page.getByText(/OrbStack · orbstack/)).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Use this engine" }).last()
  ).toBeDisabled();
});

test("search leaves total usage intact and Back restores the table filter", async ({
  page,
}) => {
  await page.goto("/?route=/dashboard/containers&scenario=docker-200&ticks=0");
  await expect(page.getByText(/200 total/)).toBeVisible();
  await page
    .getByRole("searchbox", {
      name: "Search containers by name, image or project",
    })
    .fill("worker");
  await expect(page.getByText("1 matching containers")).toBeVisible();
  await expect(page.getByText(/200 total/)).toBeVisible();
  await page
    .getByRole("table", { name: "Containers now" })
    .getByRole("link", { name: /^worker(?: ·|$)/ })
    .click();
  await expect(
    page.getByRole("heading", { name: "worker", exact: true })
  ).toBeVisible();
  await page.getByRole("button", { name: "← Containers", exact: true }).click();
  await expect(
    page.getByRole("searchbox", {
      name: "Search containers by name, image or project",
    })
  ).toHaveValue("worker");
});

// Two browsers × two themes × two viewports × six populations = 48 CPU captures,
// plus eight matching RAM captures for the representative 11-container case.
for (const theme of ["dark", "light"] as const) {
  for (const size of [
    { width: 1280, height: 900 },
    { width: 800, height: 600 },
  ]) {
    for (const count of [0, 1, 11, 32, 128, 200]) {
      test(`population screenshot ${count}, ${theme}, ${size.width}x${size.height}`, async ({
        page,
      }, info) => {
        await page.emulateMedia({ reducedMotion: "reduce" });
        await page.setViewportSize(size);
        await page.goto(
          `/?route=/dashboard/containers&scenario=docker-${count}&ticks=0&theme=${theme}`
        );
        await expect(
          page.getByRole("heading", { name: "CPU by container", exact: true })
        ).toBeVisible();
        await expect(
          page.getByRole("table", { name: "Containers now" })
        ).toBeVisible();
        await expect(
          page.getByText(`${count} matching containers`)
        ).toBeVisible();
        await expect(
          page.getByText("Loading recorded measurements…")
        ).toHaveCount(0);
        if (count === 0)
          await expect(
            page.getByText("No containers match this view.")
          ).toBeVisible();
        if (count > 100)
          expect(
            await page.locator("[data-container-id]").count()
          ).toBeLessThan(100);
        if (size.height === 900 && count >= 11) {
          const fifth = await page
            .locator("[data-container-id]")
            .nth(4)
            .boundingBox();
          expect(fifth).not.toBeNull();
          expect(
            (fifth?.y ?? Infinity) + (fifth?.height ?? 0)
          ).toBeLessThanOrEqual(size.height);
        }
        await page.screenshot({
          path: `test-results/screens/containers-${count}-${theme}-${size.width}x${size.height}-${info.project.name}.png`,
          animations: "disabled",
        });
        if (count === 11) {
          await page
            .getByRole("radio", { name: "Memory", exact: true })
            .click();
          await expect(
            page.getByRole("img", { name: /^Memory by container/ })
          ).toBeVisible();
          await expect(
            page.getByText("Loading recorded measurements…")
          ).toHaveCount(0);
          await page.screenshot({
            path: `test-results/screens/containers-memory-${theme}-${size.width}x${size.height}-${info.project.name}.png`,
            animations: "disabled",
          });
        }
      });
    }
  }
  test(`360px Activity keeps page width and text, ${theme}`, async ({
    page,
  }, info) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.setViewportSize({ width: 360, height: 760 });
    await page.goto(`${activity}&theme=${theme}`);
    await expect(
      page.getByRole("img", { name: /^CPU by container/ })
    ).toBeVisible();
    const layout = await page.locator("main").evaluate((main) => ({
      clientWidth: main.clientWidth,
      scrollWidth: main.scrollWidth,
      overflowing: [...main.querySelectorAll("*")]
        .filter(
          (element) =>
            element.getBoundingClientRect().right >
            main.getBoundingClientRect().right
        )
        .slice(0, 8)
        .map((element) => ({
          tag: element.tagName,
          text: element.textContent?.slice(0, 80),
          width: element.getBoundingClientRect().width,
        })),
    }));
    expect(layout.scrollWidth, JSON.stringify(layout)).toBeLessThanOrEqual(
      layout.clientWidth
    );
    expect(
      await page.evaluate(
        () => document.documentElement.scrollWidth <= window.innerWidth
      )
    ).toBe(true);
    await page.screenshot({
      path: `test-results/screens/containers-360-${theme}-${info.project.name}.png`,
      animations: "disabled",
      fullPage: true,
    });
    await page
      .getByRole("table", { name: "Containers now" })
      .scrollIntoViewIfNeeded();
    const firstRow = await page
      .locator('[data-container-id="container-0"]')
      .boundingBox();
    // Allow one pixel for collapsed table borders around the 44px content row.
    expect(firstRow?.height).toBeLessThanOrEqual(45);
    await page.screenshot({
      path: `test-results/screens/containers-360-table-${theme}-${info.project.name}.png`,
      animations: "disabled",
    });
  });
  test(`Activity and detail accessibility, ${theme}`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(`${activity}&theme=${theme}`);
    await expect(
      page.getByRole("img", { name: /^CPU by container/ })
    ).toBeVisible();
    expect(
      (await new AxeBuilder({ page }).include("main").analyze()).violations
    ).toEqual([]);
    await page.goto(`${detail}&theme=${theme}`);
    await expect(
      page.getByRole("heading", { name: "worker", exact: true })
    ).toBeVisible();
    await page.screenshot({
      path: `test-results/screens/container-detail-${theme}-${info.project.name}.png`,
      fullPage: true,
    });
  });
}

test("removed detail preserves evidence and logs render only plain text", async ({
  page,
}) => {
  await page.goto(detail.replace("container-0", "removed"));
  await expect(
    page.getByText(/Removed container · retained history/)
  ).toBeVisible();
  await page.getByRole("button", { name: "Open logs", exact: true }).click();
  await expect(page.getByRole("region", { name: "Log output" })).toContainText(
    "<script>literal text</script>"
  );
  await expect(
    page.getByRole("region", { name: "Log output" }).locator("script")
  ).toHaveCount(0);
  await page.getByLabel("Follow", { exact: true }).check();
  await expect(page.getByText(/Following/)).toBeVisible();
  await page.getByRole("button", { name: "Close logs" }).click();
  await expect(page.getByRole("region", { name: "Log output" })).toBeHidden();
});

test("Follow keeps connected and scrolling up exposes the new-lines count", async ({
  page,
}) => {
  await page.goto(detail);
  await page.getByRole("button", { name: "Open logs", exact: true }).click();
  await page.getByLabel("Follow", { exact: true }).check();
  const output = page.getByRole("region", { name: "Log output" });
  await expect(output).toContainText("processed job 199");
  await output.evaluate((element) => {
    element.scrollTop = 0;
  });
  await expect(
    page.getByRole("button", { name: /Autoscroll paused · [1-9]/ })
  ).toBeVisible();
  await page.getByRole("button", { name: /Jump to latest/ }).click();
  await expect(
    page.getByRole("button", { name: /Jump to latest/ })
  ).toBeHidden();
});

test("local Docker surfaces stay scoped when switching to another Mac", async ({
  page,
}) => {
  await page.goto(
    "/?route=/dashboard/containers&scenario=docker-32,multi-host&ticks=0"
  );
  await expect(
    page.getByRole("table", { name: "Containers now" })
  ).toBeVisible();
  await page.getByRole("combobox", { name: "Selected machine" }).click();
  await page.getByRole("option", { name: "Studio Mac", exact: true }).click();
  await expect(
    page.getByText(/Container monitoring is available on This Mac/)
  ).toBeVisible();
  await expect(page.getByRole("table", { name: "Containers now" })).toHaveCount(
    0
  );
  await expect(
    page.getByRole("link", { name: "Containers", exact: true })
  ).toHaveCount(0);
  await page.getByRole("link", { name: "Timeline", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "Docker timeline" })
  ).toHaveCount(0);
  await page.getByRole("combobox", { name: "Selected machine" }).click();
  await page.getByRole("option", { name: "This Mac", exact: true }).click();
  await expect(
    page.getByRole("region", { name: "Docker timeline" })
  ).toBeVisible();
  await page.getByRole("link", { name: "Containers", exact: true }).click();
  await page
    .getByRole("table", { name: "Containers now" })
    .getByRole("link", { name: /^worker(?: ·|$)/ })
    .click();
  await expect(
    page.getByRole("heading", { name: "worker", exact: true })
  ).toBeVisible();
  await page.getByRole("combobox", { name: "Selected machine" }).click();
  await page.getByRole("option", { name: "Studio Mac", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "worker", exact: true })
  ).toHaveCount(0);
  await page
    .getByRole("button", { name: "Show containers on This Mac" })
    .click();
  await expect(
    page.getByRole("heading", { name: "worker", exact: true })
  ).toBeVisible();
});

test("retained investigation remains available after disabling and on a fresh disabled startup", async ({
  page,
}, info) => {
  await page.goto(overview);
  await page.getByRole("button", { name: "Monitoring…", exact: true }).click();
  await page
    .getByRole("button", { name: "Disable monitoring", exact: true })
    .click();
  await expect(
    page.getByRole("combobox", { name: "Retained engine" })
  ).toBeVisible();
  await page.getByPlaceholder("Find retained containers").fill("Removed");
  await page.getByRole("link", { name: "Removed worker", exact: true }).click();
  await expect(page.getByText(/Last observed · CPU/)).toBeVisible();
  await page.getByRole("button", { name: "← Containers", exact: true }).click();
  await expect(page.getByPlaceholder("Find retained containers")).toHaveValue(
    "Removed"
  );
  await expect(
    page.getByRole("button", { name: "Enable monitoring" })
  ).toBeVisible();
  await page.goto(
    "/?route=/dashboard/containers&scenario=docker-retained&ticks=0"
  );
  await expect(
    page.getByRole("combobox", { name: "Retained engine" })
  ).toBeVisible();
  await page.getByRole("combobox", { name: "Retained engine" }).click();
  await page.getByRole("option", { name: "Colima · retained-colima" }).click();
  await expect(
    page.getByRole("link", { name: "Previous worker" })
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Enable monitoring" })
  ).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/docker-retained-disabled-${info.project.name}.png`,
    animations: "disabled",
  });
  await page.getByRole("link", { name: "Previous worker" }).click();
  await expect(page.getByText(/Retained engine · not selected/)).toBeVisible();
  await expect(page.getByText(/Last observed · CPU/)).toBeVisible();
});

test("task script identifies top CPU and memory, links chart focus and scopes a project", async ({
  page,
}) => {
  await page.goto(activity);
  const table = page.getByRole("table", { name: "Containers now" });
  await expect(table.getByRole("link").first()).toHaveText(/^api · liveoak$/);
  const api = table.locator('[data-container-id="container-0"]');
  const swatch = api.locator("td").first().locator("span");
  await expect(swatch).toBeVisible();
  const swatchBox = await swatch.boundingBox();
  expect(swatchBox?.width).toBe(8);
  expect(swatchBox?.height).toBe(8);
  const color = await swatch.evaluate(
    (element) => getComputedStyle(element).backgroundColor
  );
  const chart = page.getByRole("img", { name: /^CPU by container/ });
  await api.hover();
  await expect(api).toHaveClass(/bg-selected/);
  const highlighted = chart.locator('g[opacity="1"] path[stroke-width]');
  await expect(highlighted.first()).toHaveAttribute("stroke-width", "2");
  expect(
    await highlighted
      .first()
      .evaluate((element) => getComputedStyle(element).stroke)
  ).toBe(color);
  expect(await chart.locator('g[opacity="0.3"]').count()).toBeGreaterThan(0);
  await page.getByRole("radio", { name: "Memory", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "Memory by container", exact: true })
  ).toBeVisible();
  await table.getByRole("button", { name: /^Memory/ }).click();
  await expect(table.getByRole("link").first()).toHaveText(/^api · liveoak$/);
  await page.getByRole("button", { name: "api", exact: true }).focus();
  await expect(table.locator('[data-container-id="container-0"]')).toHaveClass(
    /bg-selected/
  );
  await page.getByRole("combobox", { name: "Project scope" }).click();
  await page.getByRole("option", { name: "liveoak", exact: true }).click();
  await expect(page.getByText("3 matching containers")).toBeVisible();
  await expect(
    table.getByRole("link", { name: /^api · liveoak$/ })
  ).toBeVisible();
  await expect(
    table.getByRole("link", { name: /^worker · liveoak$/ })
  ).toBeVisible();
  await expect(table.getByRole("link", { name: /^tempo/ })).toHaveCount(0);
  await expect(page.getByRole("checkbox", { name: /^Monitor / })).toHaveCount(
    0
  );
  await page.getByRole("button", { name: /^\+\d+ more$|^Choose…$/ }).click();
  const chooser = page.getByRole("dialog", { name: "Show on chart" });
  for (const checkbox of await chooser.getByRole("checkbox").all()) {
    const name = await checkbox.getAttribute("aria-label");
    const wanted = name === "Chart api" || name === "Chart worker";
    if ((await checkbox.isChecked()) !== wanted) {
      await checkbox.click();
      await expect(checkbox).toBeChecked({ checked: wanted });
    }
  }
  await expect(chooser.getByText("2 of 6", { exact: true })).toBeVisible();
  await chooser.getByRole("button", { name: "Done" }).click();
  await expect(
    page.getByRole("img", { name: /^Memory by container/ })
  ).toHaveAttribute(
    "aria-label",
    /^Memory by container, 15m, (api, worker|worker, api)$/
  );
});

test("chart chooser limits six lines without changing recording settings", async ({
  page,
}) => {
  await page.goto(activity);
  await page.getByRole("button", { name: /^\+\d+ more$/ }).click();
  const chooser = page.getByRole("dialog", { name: "Show on chart" });
  await expect(chooser.getByText("5 of 6", { exact: true })).toBeVisible();
  const candidateName = await chooser
    .getByRole("checkbox", { checked: false })
    .first()
    .getAttribute("aria-label");
  if (!candidateName) throw new Error("Chart candidate has no accessible name");
  await chooser
    .getByRole("checkbox", { name: candidateName, exact: true })
    .click();
  await expect(
    chooser.getByRole("checkbox", { name: candidateName, exact: true })
  ).toBeChecked();
  await expect(chooser.getByText("6 of 6", { exact: true })).toBeVisible();
  await expect(
    chooser.getByRole("checkbox", { checked: false }).first()
  ).toBeDisabled();
  await expect(
    chooser.getByText("Remove a container to compare another.")
  ).toBeVisible();
  await chooser.getByRole("button", { name: "Use top 5" }).click();
  await expect(chooser.getByText("5 of 6", { exact: true })).toBeVisible();
  await chooser.getByRole("button", { name: "Done" }).click();
  await page.getByRole("button", { name: "Monitoring…", exact: true }).click();
  const monitoring = page.getByRole("dialog", {
    name: "Monitoring",
    exact: true,
  });
  await expect(
    monitoring.getByRole("heading", {
      name: "Automatic collection",
      exact: true,
    })
  ).toBeVisible();
  await expect(monitoring).toContainText("up to 128 at a time");
  await expect(monitoring.getByRole("radio")).toHaveCount(0);
  await expect(
    monitoring.getByRole("checkbox", { name: /^Monitor / })
  ).toHaveCount(0);
  await expect(
    monitoring.getByRole("checkbox", { name: /^Chart / })
  ).toHaveCount(0);
});

test("historical mode includes a removed spike and opens its retained detail", async ({
  page,
}, info) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  const end = 1_791_900_000_000;
  const from = end - 900_000;
  const spikeTime = from + 220_000;
  await page.goto(
    `/?route=${encodeURIComponent(`/dashboard/containers?end=${end}`)}&scenario=docker-11&ticks=0`
  );
  await expect(
    page.getByRole("table", { name: /^Containers ·/ })
  ).toBeVisible();
  const chart = page.getByRole("img", { name: /^CPU by container/ });
  await expect(chart).toHaveAttribute("aria-label", /retired-worker/);
  const box = await chart.boundingBox();
  if (!box) throw new Error("Missing comparison plot");
  await page.mouse.move(
    box.x + box.width * (22 / 90),
    box.y + box.height * 0.2
  );
  await page.mouse.click(
    box.x + box.width * (22 / 90),
    box.y + box.height * 0.2
  );
  const tooltip = page.getByRole("tooltip").first();
  const removedLink = tooltip.getByRole("link", {
    name: "retired-worker",
    exact: true,
  });
  await expect(removedLink.locator("..")).toContainText("420%");
  const href = await removedLink.getAttribute("href");
  if (!href) throw new Error("Missing retained detail link");
  expect(new URL(href, "http://localhost").searchParams.get("end")).toBe(
    String(spikeTime + 10_000 + 300_000)
  );
  await page.screenshot({
    path: `test-results/screens/containers-historical-removed-${info.project.name}.png`,
    animations: "disabled",
  });
  await removedLink.click();
  await expect(
    page.getByRole("heading", { name: "retired-worker", exact: true })
  ).toBeVisible();
  await expect(
    page.getByText(/Removed container · retained history/)
  ).toBeVisible();
});

test("chart keyboard table alternative preserves real gaps and partial readings", async ({
  page,
}) => {
  await page.goto(activity);
  const chart = page.getByRole("img", { name: /^CPU by container/ });
  const brush = page.getByRole("slider", { name: "Select a time range" });
  await brush.focus();
  await brush.press("ArrowLeft");
  await expect(page.getByRole("tooltip").first()).toBeVisible();
  await brush.press("Escape");
  await expect(page.getByRole("tooltip")).toHaveCount(0);
  await page
    .getByRole("button", { name: "Show as table", exact: true })
    .click();
  const values = page.getByRole("table", { name: "Container history values" });
  await expect(values).toBeVisible();
  await expect(
    values.getByRole("cell", { name: /partial/ }).first()
  ).toBeAttached();
  await expect(
    values.getByRole("cell", { name: "—", exact: true }).first()
  ).toBeAttached();
  await expect(
    values.getByRole("cell", { name: "0% / 0% / 0%", exact: true }).first()
  ).toBeAttached();
  await page
    .getByRole("button", { name: "Show as chart", exact: true })
    .click();
  await expect(chart).toBeVisible();
});

for (const [scenario, notice, detailText] of [
  [
    "docker-save-failed",
    "Recent history is not being saved. Kelvo will retry.",
    "Measurements could not be saved",
  ],
  [
    "docker-history-gap",
    "CPU history missing for 1 interval",
    "Kelvo was not running",
  ],
  [
    "docker-events-gap",
    "Some container events may be missing",
    "Lost connection to Docker events",
  ],
] as const) {
  test(`one history notice explains ${scenario}`, async ({ page }, info) => {
    await page.goto(
      `/?route=/dashboard/containers&scenario=docker-11,${scenario}&ticks=0`
    );
    await expect(page.getByText(notice, { exact: true })).toBeVisible();
    await page.getByRole("button", { name: /^Details \(/ }).click();
    await expect(
      page.getByRole("dialog", { name: "History details" })
    ).toContainText(detailText);
    await page.screenshot({
      path: `test-results/screens/containers-${scenario}-${info.project.name}.png`,
      animations: "disabled",
    });
  });
}

test("row actions show in-flight states, keep rows fixed and show a failed Start", async ({
  page,
}, info) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto(activity);
  await expect(
    page.getByRole("table", { name: "Containers now" })
  ).toBeVisible();
  await page.clock.install();
  await page.clock.pauseAt(new Date(Date.now() + 1000));
  const api = page.locator('[data-container-id="container-0"]');
  await api.hover();
  const before = await api.boundingBox();
  await api.getByRole("button", { name: "Stop api", exact: true }).click();
  await expect(page.getByRole("alertdialog")).toContainText("30 seconds");
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "Stop", exact: true })
    .click();
  await expect(api.getByText("Stopping…", { exact: true })).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/containers-stop-inflight-${info.project.name}.png`,
    animations: "disabled",
  });
  await api.hover();
  await page.clock.runFor(750);
  await expect(
    api.getByRole("button", { name: "Start api", exact: true })
  ).toBeVisible();
  expect((await api.boundingBox())?.y).toBe(before?.y);
  await api.getByRole("button", { name: "Start api", exact: true }).click();
  await expect(api.getByText("Starting…", { exact: true })).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/containers-start-inflight-${info.project.name}.png`,
    animations: "disabled",
  });
  await page.clock.runFor(750);
  await expect(
    api.getByRole("button", { name: "Stop api", exact: true })
  ).toBeVisible();
  const worker = page.locator('[data-container-id="container-1"]');
  await worker.hover();
  await worker
    .getByRole("button", { name: "Stop worker", exact: true })
    .click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "Stop", exact: true })
    .click();
  await page.clock.runFor(750);
  await worker
    .getByRole("button", { name: "Start worker", exact: true })
    .click();
  await page.clock.runFor(750);
  await expect(
    worker.getByText("Port 8080 is already in use", { exact: true })
  ).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/containers-start-failed-${info.project.name}.png`,
    animations: "disabled",
  });
  await page.clock.runFor(10_000);
  await expect(
    worker.getByText("Port 8080 is already in use", { exact: true })
  ).toHaveCount(0);
});

test("Monitoring follows automatic inventory and has only display preferences", async ({
  page,
}, info) => {
  await page.goto(activity);
  await page.getByRole("button", { name: "Monitoring…", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Monitoring", exact: true });
  await expect(
    dialog.getByRole("heading", { name: "Automatic collection", exact: true })
  ).toBeVisible();
  await expect(dialog).toContainText("Docker is the source of truth");
  await expect(dialog).toContainText("up to 128 at a time");
  await expect(dialog.getByRole("checkbox")).toHaveCount(2);
  await expect(
    dialog.getByRole("checkbox", { name: "Show in popover", exact: true })
  ).toBeVisible();
  await expect(
    dialog.getByRole("checkbox", {
      name: "Show count in menu bar",
      exact: true,
    })
  ).toBeVisible();
  await expect(dialog.getByRole("radio")).toHaveCount(0);
  await expect(dialog.getByRole("checkbox", { name: /^Monitor / })).toHaveCount(
    0
  );
  await expect(
    dialog.getByRole("button", { name: /^(Apply|Cancel)$/ })
  ).toHaveCount(0);
  await page.screenshot({
    path: `test-results/screens/containers-automatic-monitoring-${info.project.name}.png`,
    animations: "disabled",
  });
  await dialog.getByRole("button", { name: "Done", exact: true }).click();
  await expect(dialog).toHaveCount(0);
  await expect(
    page.getByText("5 running · 11 total", { exact: true })
  ).toBeVisible();
  const redis = page.locator('[data-container-id="container-5"]');
  await redis.getByRole("button", { name: "Start redis", exact: true }).click();
  await expect(
    redis.getByRole("button", { name: "Stop redis", exact: true })
  ).toBeVisible();
  await expect(
    page.getByText("6 running · 11 total", { exact: true })
  ).toBeVisible();
  await page.getByRole("button", { name: "Monitoring…", exact: true }).click();
  await expect(
    dialog.getByRole("heading", { name: "Automatic collection", exact: true })
  ).toBeVisible();
  await expect(dialog.getByRole("checkbox")).toHaveCount(2);
  await page.goto(`${activity}&theme=dark`);
  await page.getByRole("button", { name: "Monitoring…", exact: true }).click();
  await expect(
    dialog.getByRole("heading", { name: "Automatic collection", exact: true })
  ).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/containers-automatic-monitoring-dark-${info.project.name}.png`,
    animations: "disabled",
  });
});

test("shared row context menu uses the current running or exited container", async ({
  page,
}) => {
  await page.goto(activity);
  const api = page.locator('[data-container-id="container-0"]');
  await api.getByRole("link").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Stop", exact: true }).click();
  await expect(page.getByRole("alertdialog")).toContainText("Stop api?");
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "Cancel", exact: true })
    .click();
  const redis = page.locator('[data-container-id="container-5"]');
  await redis.getByRole("link").click({ button: "right" });
  await expect(
    page.getByRole("menuitem", { name: "Stop", exact: true })
  ).toHaveCount(0);
  await page.getByRole("menuitem", { name: "Start", exact: true }).click();
  await expect(
    redis.getByRole("button", { name: "Stop redis", exact: true })
  ).toBeVisible();
  await expect(
    api.getByRole("button", { name: "Stop api", exact: true })
  ).toBeVisible();
  await expect(
    page.getByText("6 running · 11 total", { exact: true })
  ).toBeVisible();
  const worker = page.locator('[data-container-id="container-1"]');
  await worker.getByRole("link").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Stop", exact: true }).click();
  await expect(page.getByRole("alertdialog")).toContainText("Stop worker?");
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "Cancel", exact: true })
    .click();
});

test("View controls grouping while held order adds no status copy", async ({
  page,
}) => {
  await page.goto(activity);
  const table = page.getByRole("table", { name: "Containers now" });
  await table.getByRole("link").first().focus();
  await expect(page.getByText(/Order paused|Reorder now/)).toHaveCount(0);
  await page.getByRole("button", { name: "View", exact: true }).click();
  await page
    .getByRole("menuitemcheckbox", { name: "Group by project", exact: true })
    .click();
  await expect(
    table.getByRole("button", { name: /liveoak.*3 containers/ })
  ).toBeVisible();
  await table.getByRole("button", { name: /liveoak.*3 containers/ }).click();
  await expect(
    table.getByRole("link", { name: /^api · liveoak$/ })
  ).toHaveCount(0);
  await table
    .getByRole("button", { name: /liveoak.*3 containers/ })
    .press("Enter");
  await expect(
    table.getByRole("link", { name: /^api · liveoak$/ })
  ).toBeVisible();
});

test("a chart brush scopes the historical table and Live clears it", async ({
  page,
}) => {
  await page.goto(activity);
  const chart = page.getByRole("img", { name: /^CPU by container/ });
  await expect(chart).toBeVisible();
  const bounds = await chart.boundingBox();
  if (!bounds) throw new Error("Comparison chart has no bounds");
  await page.mouse.move(
    bounds.x + bounds.width * 0.2,
    bounds.y + bounds.height / 2
  );
  await page.mouse.down();
  await page.mouse.move(
    bounds.x + bounds.width * 0.6,
    bounds.y + bounds.height / 2,
    { steps: 8 }
  );
  await page.mouse.up();
  await expect(
    page.getByRole("table", { name: /^Containers ·/ })
  ).toBeVisible();
  await expect(
    page
      .getByRole("table", { name: /^Containers ·/ })
      .getByRole("columnheader", { name: /Peak CPU/ })
  ).toBeVisible();
  await page.getByRole("button", { name: "Live", exact: true }).click();
  await expect(
    page.getByRole("table", { name: "Containers now", exact: true })
  ).toBeVisible();
});

test("failed Stop keeps the container running and explains the outcome", async ({
  page,
}, info) => {
  await page.goto(
    "/?route=/dashboard/containers&scenario=docker-11,docker-stop-failed&ticks=0"
  );
  const api = page.locator('[data-container-id="container-0"]');
  await expect(api).toBeVisible();
  await page.clock.install();
  await page.clock.pauseAt(new Date(Date.now() + 1000));
  await api.hover();
  await api.getByRole("button", { name: "Stop api", exact: true }).click();
  await page
    .getByRole("alertdialog")
    .getByRole("button", { name: "Stop", exact: true })
    .click();
  await page.clock.runFor(750);
  await expect(
    api.getByText("Docker could not stop this container", { exact: true })
  ).toBeVisible();
  await expect(
    api.getByRole("button", { name: "Stop api", exact: true })
  ).toBeVisible();
  await page.screenshot({
    path: `test-results/screens/containers-stop-failed-${info.project.name}.png`,
    animations: "disabled",
  });
});

test("an expensive newcomer is visible in the table before automatic chart promotion", async ({
  page,
}, info) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.clock.install({ time: new Date("2026-10-13T18:00:00Z") });
  await page.clock.pauseAt(new Date("2026-10-13T18:00:01Z"));
  await page.goto(
    "/?route=/dashboard/containers&scenario=docker-11,docker-rerank"
  );
  const chart = page.getByRole("img", { name: /^CPU by container/ });
  await expect(chart).toBeVisible();
  await expect(chart).not.toHaveAttribute("aria-label", /tempo/);
  // Three separate boundary observations are required; a single jump is one evaluation.
  await page.clock.runFor(10_000);
  await page.clock.runFor(10_000);
  const table = page.getByRole("table", { name: "Containers now" });
  await expect(table.getByRole("link").first()).toHaveText("tempo · opendata");
  await expect(
    table.locator('[data-container-id="container-4"]')
  ).toContainText("450%");
  await expect(
    page.getByRole("button", { name: /^\+\d+ more$/ })
  ).toBeVisible();
  await page.clock.runFor(10_000);
  await page.clock.runFor(10_000);
  // TanStack Query publishes the new selected series on a queued notification.
  await page.clock.runFor(100);
  await expect(chart).toHaveAttribute("aria-label", /tempo/);
  await page.screenshot({
    path: `test-results/screens/containers-new-top-consumer-${info.project.name}.png`,
    animations: "disabled",
  });
});

test("need attention reveals a filtered container in a collapsed project", async ({
  page,
}) => {
  const route =
    "/dashboard/containers?q=not-found&state=exited&group=grouped&collapsed=project-1";
  await page.goto(
    `/?route=${encodeURIComponent(route)}&scenario=docker-32&ticks=0`
  );
  await expect(page.getByText("0 matching containers")).toBeVisible();
  await page.getByRole("button", { name: /need attention/ }).click();
  await expect(
    page.getByRole("searchbox", {
      name: "Search containers by name, image or project",
    })
  ).toHaveValue("");
  const row = page.locator('[data-container-id="container-0"]');
  await expect(row).toBeVisible();
  await expect(row).toHaveClass(/bg-selected/);
  await expect(
    page.getByRole("button", { name: /project-1.*containers/ }).first()
  ).toHaveAttribute("aria-expanded", "true");
});

for (const via of ["table", "tooltip", "keyboard"] as const) {
  test(`container detail Back restores comparison and scroll via ${via}`, async ({
    page,
  }) => {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.setViewportSize({ width: 1280, height: 700 });
    const route =
      "/dashboard/containers?project=&q=service&chart=container-4,container-9&metric=memory&range=1h&end=1791900000000";
    await page.goto(
      `/?route=${encodeURIComponent(route)}&scenario=docker-200&ticks=0`
    );
    const table = page.getByRole("table", { name: /^Containers ·/ });
    const scroller = table.locator("..");
    const chart = page.getByRole("img", { name: /^Memory by container, 1h/ });
    await expect(chart).toHaveAttribute("aria-label", /service-4, service-9$/);
    const title = await table.getAttribute("aria-label");
    await scroller.evaluate((element) => {
      element.scrollTop = 150;
    });
    if (via === "table") {
      await table.scrollIntoViewIfNeeded();
      await page.locator("main").evaluate((element) => {
        element.scrollTop += 40;
      });
    } else {
      const brush = page.getByRole("slider", { name: "Select a time range" });
      await brush.focus();
      await brush.press("ArrowLeft");
      await brush.press("Enter");
      // Select enough recorded time to keep the history table populated (the
      // mock deliberately includes a gap in each retained range).
      for (let i = 0; i < 10; i++) await brush.press("Shift+ArrowLeft");
      await expect(table.getByRole("link").first()).toBeVisible();
      await page.locator("main").evaluate((element) => {
        element.scrollTop = 100;
      });
    }
    // Selecting through the shared brush refreshes the historical table.
    await scroller.evaluate((element) => {
      element.scrollTop = 150;
    });
    const saved = await page.evaluate(() => ({
      main: document.querySelector("main")?.scrollTop ?? 0,
      table:
        document.querySelector("table[aria-label^='Containers ·']")
          ?.parentElement?.scrollTop ?? 0,
    }));
    expect(saved.main).toBeGreaterThan(0);
    expect(saved.table).toBeGreaterThan(0);
    if (via === "table") {
      // Dispatch after scrolling so no intermediate hover/focus render refreshes a stale Link state.
      await table
        .getByRole("link")
        .first()
        .dispatchEvent("click", { button: 0 });
    } else if (via === "tooltip") {
      await page
        .getByRole("tooltip")
        .getByRole("link", { name: "service-4", exact: true })
        .dispatchEvent("click", { button: 0 });
    } else {
      await page
        .getByRole("tooltip")
        .getByRole("link", { name: "service-4", exact: true })
        .press("Enter");
    }
    await expect(
      page.getByRole("button", { name: "← Containers", exact: true })
    ).toBeVisible();
    await page
      .getByRole("button", { name: "← Containers", exact: true })
      .click();
    await expect(table).toHaveAttribute("aria-label", title ?? "");
    await expect(
      page.getByRole("searchbox", {
        name: "Search containers by name, image or project",
      })
    ).toHaveValue("service");
    await expect(
      page.getByRole("combobox", { name: "Project scope" })
    ).toContainText("Standalone");
    await expect(
      page
        .getByRole("radiogroup", { name: "Container history range" })
        .getByRole("radio", { name: "1h", exact: true })
    ).toHaveAttribute("aria-checked", "true");
    await expect(chart).toHaveAttribute("aria-label", /service-4, service-9$/);
    await expect
      .poll(() => page.locator("main").evaluate((element) => element.scrollTop))
      .toBe(saved.main);
    await expect
      .poll(() => scroller.evaluate((element) => element.scrollTop))
      .toBe(saved.table);
  });
}

for (const metric of ["cpu", "memory"] as const) {
  test(`container ${metric} selection keeps the plot, supports hover and shares all clear gestures`, async ({
    page,
  }, info) => {
    const end = 1791900000000;
    await page.goto(
      `/?route=${encodeURIComponent(`/dashboard/containers?metric=${metric}&chart=container-0&end=${end}`)}&scenario=docker-11&ticks=0`
    );
    const chart = page.getByRole("img", { name: /by container,/ });
    await expect(chart).toHaveAttribute("aria-label", /api$/);
    const paths = () =>
      chart
        .locator("g path")
        .evaluateAll((nodes) => nodes.map((node) => node.getAttribute("d")));
    await expect.poll(async () => (await paths()).length).toBeGreaterThan(0);
    const before = await paths();
    await expect(page.getByText(/10 s average · shaded min–max/)).toBeVisible();
    await expect(chart.locator("path[fill-opacity]")).toBeVisible();
    const brush = page.getByRole("slider", { name: "Select a time range" });
    const box = await brush.boundingBox();
    if (!box) throw new Error("Missing comparison brush");
    const drag = async () => {
      await page.mouse.move(box.x + box.width * 0.1, box.y + box.height * 0.5);
      await page.mouse.down();
      await page.mouse.move(
        box.x + box.width * 0.25,
        box.y + box.height * 0.5,
        { steps: 8 }
      );
      await page.mouse.up();
      await expect(
        page.getByRole("button", { name: "Clear selection" })
      ).toBeVisible();
      await expect.poll(paths).toEqual(before);
    };
    await drag();
    await page.mouse.move(box.x + box.width * 0.75, box.y + box.height * 0.5);
    const tip = page.getByRole("tooltip");
    await expect(tip).toBeVisible();
    await expect(tip).toContainText("min / avg / max");
    const text = await tip.textContent();
    await page.mouse.move(box.x + box.width * 0.9, box.y + box.height * 0.5);
    await expect(tip).not.toHaveText(text ?? "");
    await page.screenshot({
      path: `test-results/screens/containers-${metric}-selected-hover-${info.project.name}.png`,
      animations: "disabled",
    });
    await page.mouse.click(box.x + box.width * 0.9, box.y + box.height * 0.5);
    await expect(page.getByTestId("brush-band")).toHaveCount(0);
    await drag();
    await brush.press("Escape");
    await expect(page.getByTestId("brush-band")).toHaveCount(0);
    await drag();
    await page.getByRole("button", { name: "Clear selection" }).click();
    await expect(page.getByTestId("brush-band")).toHaveCount(0);
    await drag();
    await page
      .getByRole("heading", { name: "Containers", exact: true })
      .click();
    await expect(page.getByTestId("brush-band")).toHaveCount(0);
    await expect.poll(paths).toEqual(before);
  });
}

for (const metric of ["cpu", "memory"] as const) {
  test(`container ${metric} statistics tooltip fits a narrow window`, async ({
    page,
  }, info) => {
    await page.setViewportSize({ width: 360, height: 900 });
    await page.goto(
      `/?route=${encodeURIComponent(`/dashboard/containers?metric=${metric}&chart=container-0&end=1791900000000`)}&scenario=docker-11&ticks=0`
    );
    const brush = page.getByRole("slider", { name: "Select a time range" });
    await brush.focus();
    await brush.press("ArrowLeft");
    const tooltip = page.getByRole("tooltip");
    await expect(tooltip).toContainText("min / avg / max");
    expect(
      await tooltip.evaluate(
        (element) => element.scrollWidth <= element.clientWidth + 1
      )
    ).toBe(true);
    await page.screenshot({
      path: `test-results/screens/containers-${metric}-statistics-narrow-${info.project.name}.png`,
      animations: "disabled",
    });
  });
}
