import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Toaster } from "~/components/ui/sonner";
import { POPOVER_SERIES } from "./_lib/series";
import PopoverRoute from "./route";

const NOW = 1_800_000_000_000;

/** The popover's text after one frame, with or without the projection. */
async function popoverText(projected: boolean) {
  const r = renderWithProviders(<PopoverRoute />, {
    transportOptions: { now: () => NOW, scenarios: ["cpu-power-seeded"] },
    series: projected ? POPOVER_SERIES : undefined,
  });
  await screen.findByText("Open dashboard");
  act(() => r.transport.tick());
  const text = document.body.textContent ?? "";
  r.unmount();
  return text;
}

describe("popover series projection (D-066)", () => {
  it("reads the same on the projected channel as on every series", async () => {
    const full = await popoverText(false);
    const projected = await popoverText(true);
    // A metric the projection left out would read as a gap here.
    expect(projected).toBe(full);
    expect(full).toContain("estimated from last calibration");
  });
});

describe("popover cards", () => {
  it("open their module page in the dashboard", async () => {
    const { transport, user } = renderWithProviders(<PopoverRoute />, {
      transportOptions: { now: () => NOW },
    });
    await screen.findByText("Open dashboard");
    act(() => transport.tick());
    await user.click(screen.getByRole("link", { name: "CPU" }));
    expect(screen.queryByRole("link", { name: "Cores" })).toBeNull();
    expect(
      screen.getByRole("link", { name: "Power & battery" })
    ).toHaveTextContent("Battery");
    expect(screen.queryByRole("link", { name: "Battery" })).toBeNull();
    await user.click(screen.getByRole("link", { name: "Memory" }));
    const opened = transport.calls
      .filter((c) => c.command === "open_dashboard")
      .map((c) => c.args[0]);
    expect(opened).toEqual(["/dashboard/cpu", "/dashboard/memory"]);
  });
});

describe("saved card presentation", () => {
  it("collapses independently without navigation and restores the saved body on remount", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    const first = renderWithProviders(<PopoverRoute />, { transport });
    await screen.findByRole("button", { name: "Collapse CPU" });
    await first.user.click(
      screen.getByRole("button", { name: "Collapse CPU" })
    );
    await screen.findByRole("button", { name: "Expand CPU" });
    expect(screen.queryByText("User", { exact: true })).toBeNull();
    expect(
      screen.getByRole("button", { name: "Collapse GPU" })
    ).toHaveAttribute("aria-expanded", "true");
    expect(
      transport.calls.filter((c) => c.command === "open_dashboard")
    ).toHaveLength(0);
    first.unmount();
    const second = renderWithProviders(<PopoverRoute />, { transport });
    await screen.findByRole("button", { name: "Expand CPU" });
    expect(screen.queryByText("User", { exact: true })).toBeNull();
    await second.user.click(screen.getByRole("link", { name: "CPU" }));
    expect(
      transport.calls.filter((c) => c.command === "open_dashboard").at(-1)
        ?.args[0]
    ).toBe("/dashboard/cpu");
  });

  it("Collapse all writes hidden card preferences too, and save failures retain expanded cards", async () => {
    const first = renderWithProviders(<PopoverRoute />, {
      transportOptions: { now: () => NOW },
    });
    await screen.findByRole("button", { name: "Collapse CPU" });
    await first.user.click(
      screen.getByRole("button", { name: "Popover options" })
    );
    await first.user.click(
      await screen.findByRole("menuitem", { name: "Collapse all" })
    );
    await screen.findByRole("button", { name: "Expand CPU" });
    const settings = await first.transport.getSettings();
    expect(Object.values(settings.settings.popover ?? {})).toEqual(
      Array(7).fill("compact")
    );
    first.unmount();
    const failed = renderWithProviders(<PopoverRoute />, {
      transportOptions: { now: () => NOW, settingsNotSaved: true },
    });
    await screen.findByRole("button", { name: "Collapse CPU" });
    await failed.user.click(
      screen.getByRole("button", { name: "Collapse CPU" })
    );
    await waitFor(() =>
      expect(
        failed.transport.calls.some((c) => c.command === "update_settings")
      ).toBe(true)
    );
    expect(screen.queryByRole("button", { name: "Expand CPU" })).toBeNull();
    expect(
      screen.getByRole("button", { name: "Collapse CPU" })
    ).toHaveAttribute("aria-expanded", "true");
  });
});

it("shows one retryable save error and applies the original choice after recovery", async () => {
  const options = { now: () => NOW, autoTick: false, settingsNotSaved: true };
  const transport = createMockTransport(options);
  const r = renderWithProviders(
    <>
      <PopoverRoute />
      <Toaster />
    </>,
    { transport }
  );
  await screen.findByRole("button", { name: "Collapse CPU" });
  await r.user.click(screen.getByRole("button", { name: "Collapse CPU" }));
  await screen.findByText("Popover options could not be saved.");
  expect(screen.getByRole("button", { name: "Collapse CPU" })).toHaveAttribute(
    "aria-expanded",
    "true"
  );
  options.settingsNotSaved = false;
  await r.user.click(screen.getByRole("button", { name: "Retry" }));
  await screen.findByRole("button", { name: "Expand CPU" });
  expect(screen.queryByText("User", { exact: true })).toBeNull();
});

it("waits for saved density before rendering any card body", async () => {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  await transport.updateSettings({ popover: { cpu: "compact" } });
  const snapshot = await transport.getSettings();
  let release = () => {};
  transport.getSettings = () =>
    new Promise((resolve) => {
      release = () => resolve(snapshot);
    });
  renderWithProviders(<PopoverRoute />, { transport });
  expect(screen.queryByRole("link", { name: "CPU" })).toBeNull();
  await act(async () => release());
  await screen.findByRole("button", { name: "Expand CPU" });
  expect(screen.queryByText("User", { exact: true })).toBeNull();
});

it("keeps sensor failure and missing values visible when compact", async () => {
  const r = renderWithProviders(<PopoverRoute />, {
    transportOptions: { now: () => NOW, scenarios: ["popover-compact"] },
  });
  await screen.findByRole("button", { name: "Expand CPU" });
  act(() => {
    r.transport.push({
      kind: "layout",
      layout_no: 99,
      kinds: ["gauge"],
      series: [{ metric: "cpu.total", labels: [] }],
    });
    r.transport.push({
      kind: "frame",
      layout_no: 99,
      timeline: 0,
      ts_ms: NOW + 1_000,
      values: [null],
      held: [null],
    });
  });
  await screen.findByText(/Sensor read failed/);
  expect(screen.getByRole("link", { name: "CPU" })).toHaveTextContent("—");
  expect(
    screen.getByRole("button", { name: "Expand CPU" })
  ).toBeInTheDocument();
});
