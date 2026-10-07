import type { ProcessView } from "@core/generated/bindings";
import { BACKGROUND_PROCESSES, PROCESSES } from "@core/mock/fixtures";
import { act, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import GpuRoute from "./route";

const views = (calls: { command: string; args: unknown[] }[]) =>
  calls
    .filter((c) => c.command === "set_process_interest")
    .map((c) => c.args[2] as ProcessView | null);

describe("GPU page processes (D-085)", () => {
  it("lists your processes by GPU share and asks for the top 12 every sample", async () => {
    const { transport } = renderWithProviders(<GpuRoute />);
    const table = await screen.findByRole("table", { name: "GPU by process" });
    expect(views(transport.calls).at(-1)).toEqual({
      limit: 12,
      sort: ["gpu"],
      period_ms: null,
      gpu: true,
    });
    const rows = within(table).getAllByRole("row").slice(1);
    // Processes without GPU time are left out; the rest rank by share.
    expect(
      rows.map((r) => within(r).getAllByRole("cell")[0]?.textContent)
    ).toEqual([
      "WWindowServer",
      "FFigma",
      "SSafari",
      "XXcode",
      "ccom.docker.backend",
      "SSafari Web Content",
      "KKelvo",
      "DDock",
    ]);
    expect(rows[0]?.textContent).toContain("14.2");
    expect(screen.getByText(/long GPU compute job/)).toBeVisible();
  });

  it("grows the table with its rows and never shrinks it at 1 Hz", async () => {
    const { transport } = renderWithProviders(<GpuRoute />);
    await screen.findByRole("table", { name: "GPU by process" });
    // The table's box: body, header row, 4 px.
    const box = () => screen.getByTestId("process-table-scroll").style.height;
    const batch = (n: number) =>
      [...PROCESSES, ...BACKGROUND_PROCESSES]
        .slice(0, n)
        .map((p) => ({ ...p, gpu_pct: 1 }));
    const quiet = PROCESSES.map((p) => ({ ...p, gpu_pct: 0 }));
    await vi.waitFor(() => expect(box()).toBe(`${9 * 29 + 4}px`));
    // An empty batch keeps the table and its height, with a message row.
    act(() => transport.push({ kind: "processes", ts_ms: 1, rows: quiet }));
    expect(screen.getByText(/None of your processes/)).toBeVisible();
    expect(box()).toBe(`${9 * 29 + 4}px`);
    act(() => transport.push({ kind: "processes", ts_ms: 1, rows: batch(5) }));
    expect(
      within(
        screen.getByRole("table", { name: "GPU by process" })
      ).getAllByRole("row")
    ).toHaveLength(6);
    expect(box()).toBe(`${9 * 29 + 4}px`);
    act(() => transport.push({ kind: "processes", ts_ms: 2, rows: batch(10) }));
    expect(box()).toBe(`${11 * 29 + 4}px`);
  });

  it("says it is measuring while a batch carries no GPU time (the baseline)", async () => {
    const { transport } = renderWithProviders(<GpuRoute />);
    await screen.findByRole("table", { name: "GPU by process" });
    act(() => transport.push({ kind: "processes", ts_ms: 1, rows: PROCESSES }));
    expect(await screen.findByText("Measuring GPU by process…")).toBeVisible();
  });

  it("has no process section when the host cannot attribute GPU time", async () => {
    const { transport } = renderWithProviders(<GpuRoute />, {
      transportOptions: { scenarios: ["no-process-gpu"] },
    });
    await screen.findByRole("heading", { name: "GPU" });
    expect(screen.queryByRole("heading", { name: "Processes" })).toBeNull();
    expect(views(transport.calls)).toEqual([]);
  });
});
