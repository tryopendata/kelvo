import { createMockTransport } from "@core/mock-transport";
import { act, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Route, Routes } from "react-router";
import DiskRoute from "../disk/route";
import GpuRoute from "../gpu/route";
import CpuRoute from "./route";

function Pages() {
  return (
    <Routes>
      <Route path="cpu" element={<CpuRoute />} />
      <Route path="gpu" element={<GpuRoute />} />
      <Route path="disk" element={<DiskRoute />} />
    </Routes>
  );
}

describe("CPU page chart window (D-091)", () => {
  it("applies a window picked on CPU to its charts and to the GPU and Disk pages", async () => {
    const { router, transport, user } = renderWithProviders(<Pages />, {
      route: "/cpu",
    });
    await user.click(await screen.findByRole("radio", { name: "30m" }));
    expect(
      await screen.findByRole("img", {
        name: /^CPU total and system, last 30 minutes,/,
      })
    ).toBeVisible();
    expect(
      screen.getByRole("heading", { name: "Per-core load, last 30 minutes" })
    ).toBeVisible();
    expect(
      transport.calls
        .filter((c) => c.command === "update_settings")
        .map((c) => c.args[0])
    ).toEqual([{ general: { chart_window: "30m" } }]);

    await act(() => router.navigate("/gpu"));
    expect(
      await screen.findByRole("heading", { name: "Power, last 30 minutes" })
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: "30m" })).toBeChecked();
    expect(
      screen.getByRole("img", { name: /^GPU power, last 30 minutes,/ })
    ).toBeVisible();

    await act(() => router.navigate("/disk"));
    expect(
      await screen.findByRole("img", { name: /last 30 minutes/ })
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: "30m" })).toBeChecked();
  });

  it("draws at the default 15m when settings could not be read", async () => {
    const transport = createMockTransport({ autoTick: false });
    transport.getSettings = () => Promise.reject(new Error("bridge down"));
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    onTestFinished(() => error.mockRestore());
    renderWithProviders(<CpuRoute />, { transport });
    expect(
      await screen.findByRole("img", {
        name: /^CPU total and system, last 15 minutes,/,
      })
    ).toBeVisible();
    expect(screen.getByRole("radio", { name: "15m" })).toBeChecked();
    expect(error).toHaveBeenCalledWith(
      "[settings] get_settings failed",
      expect.anything()
    );
  });
});

const T = 1_700_000_000_000; // a 10 s edge
const NOW = T + 5000;
const S = 1000;

const usageCalls = (calls: { command: string; args: unknown[] }[]) =>
  calls
    .filter((c) => c.command === "query_usage_by_app")
    .map((c) => [c.args[1], c.args[2], c.args[3]]);
const statsCalls = (calls: { command: string; args: unknown[] }[]) =>
  calls
    .filter((c) => c.command === "query_series_stats")
    .map((c) => [c.args[1], c.args[2], c.args[3]]);
const appsTable = () => screen.findByRole("table", { name: "CPU by app" });
const names = (table: HTMLElement) =>
  within(table)
    .getAllByRole("row")
    .slice(1)
    .map((r) => within(r).queryAllByRole("cell")[0]?.textContent ?? "");

function renderCpu(clock: () => number = () => NOW) {
  const transport = createMockTransport({
    now: clock,
    autoTick: false,
    chartWindow: "5m",
  });
  return renderWithProviders(<CpuRoute />, { transport, backfillMs: 600_000 });
}

describe("CPU page over a range (D-099)", () => {
  it("lists apps by average CPU over the window, with what the host did beyond them", async () => {
    const { transport } = renderCpu();
    const table = await appsTable();
    expect(
      screen.getByRole("heading", { name: "CPU by app, last 5 minutes" })
    ).toBeVisible();
    const rows = names(table);
    expect(rows[0]).toMatch(/^XXcode/);
    expect(rows).toContain("System and other");
    expect(
      within(table).getByRole("columnheader", { name: /Avg CPU/ })
    ).toHaveAttribute("aria-sort", "descending");
    // The window ends where the open 10 s bucket starts.
    expect(usageCalls(transport.calls)).toEqual([[T - 300 * S, T, "cpu"]]);
    expect(statsCalls(transport.calls)).toEqual([
      [["cpu.total"], T - 300 * S, T],
    ]);
    await vi.waitFor(() =>
      expect(screen.getByTestId("cpu-range-totals")).toHaveTextContent(
        /Avg · 5 min[\d.]+%Peak · 5 min\d+%/
      )
    );
    // The live process table is gone, and with it the 1 s process rows.
    expect(
      transport.calls.filter((c) => c.command === "set_process_interest")
    ).toEqual([]);
  });

  it("reads the window once per 10 s bucket, not every tick", async () => {
    let clock = NOW;
    const { transport } = renderCpu(() => clock);
    await appsTable();
    const before = usageCalls(transport.calls).length;
    for (let i = 0; i < 4; i++) {
      clock += 1000;
      act(() => transport.tick());
    }
    await act(async () => {});
    expect(usageCalls(transport.calls).length).toBe(before);
    for (let i = 0; i < 2; i++) {
      clock += 1000;
      act(() => transport.tick());
    }
    await vi.waitFor(() =>
      expect(usageCalls(transport.calls)).toContainEqual([
        T - 290 * S,
        T + 10 * S,
        "cpu",
      ])
    );
  });

  it("a brushed range scopes the totals and the table; Esc and a press elsewhere clear it", async () => {
    const { transport, user } = renderCpu();
    await appsTable();
    const brush = screen.getByRole("slider", { name: "Select a time range" });
    act(() => brush.focus());
    // Focus starts on [T − 10 s, T); one left, select, extend right.
    await user.keyboard("{ArrowLeft}{Enter}{Shift>}{ArrowRight}{/Shift}");
    expect(
      await screen.findByRole("heading", { name: "CPU by app, selected 20 s" })
    ).toBeVisible();
    expect(usageCalls(transport.calls)).toContainEqual([T - 20 * S, T, "cpu"]);
    await vi.waitFor(() =>
      expect(screen.getByTestId("cpu-range-totals")).toHaveTextContent(
        /Avg · 20 s/
      )
    );
    expect(screen.getByTestId("stream-veil")).toBeInTheDocument();
    expect(screen.getByTestId("selection-summary")).toBeVisible();

    await user.keyboard("{Escape}");
    expect(
      await screen.findByRole("heading", { name: "CPU by app, last 5 minutes" })
    ).toBeVisible();
    expect(screen.queryByTestId("stream-veil")).toBeNull();

    act(() => brush.focus());
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", { name: "CPU by app, selected 10 s" })
    ).toBeVisible();
    // The per-core heatmap stays live; a press on it is outside the selection.
    await user.click(
      screen.getByRole("heading", { name: "Per-core load, last 5 minutes" })
    );
    expect(
      await screen.findByRole("heading", { name: "CPU by app, last 5 minutes" })
    ).toBeVisible();
  });

  it("expands an app to its processes", async () => {
    const { user } = renderCpu();
    const table = await appsTable();
    await user.click(
      within(table).getByRole("button", { name: "Show Safari processes" })
    );
    expect(
      within(table).getByRole("button", { name: "Hide Safari processes" })
    ).toHaveAttribute("aria-expanded", "true");
    expect(names(table).some((n) => /^Safari Web Content\d+$/.test(n))).toBe(
      true
    );
  });
});
