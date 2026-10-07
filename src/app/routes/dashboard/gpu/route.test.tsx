import { createMockTransport } from "@core/mock-transport";
import { act, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import GpuRoute from "./route";

const T = 1_700_000_000_000; // a 10 s edge
const NOW = T + 5000;
const S = 1000;

const calls = (c: { command: string; args: unknown[] }[], command: string) =>
  c.filter((x) => x.command === command).map((x) => x.args.slice(1));

function render(scenarios?: string[]) {
  const transport = createMockTransport({
    now: () => NOW,
    autoTick: false,
    chartWindow: "5m",
    ...(scenarios ? { scenarios: scenarios as never } : {}),
  });
  return renderWithProviders(<GpuRoute />, { transport, backfillMs: 600_000 });
}

describe("GPU page over a range (D-085, D-099)", () => {
  it("lists apps by average share of the GPU over the window, with the rest of utilization", async () => {
    const { transport } = render();
    const table = await screen.findByRole("table", { name: "GPU by app" });
    expect(
      screen.getByRole("heading", { name: "GPU by app, last 5 minutes" })
    ).toBeVisible();
    expect(
      within(table).getByRole("columnheader", { name: /Avg GPU/ })
    ).toHaveAttribute("aria-sort", "descending");
    expect(within(table).getByText("System and other")).toBeVisible();
    expect(within(table).getByText(/long GPU compute job/)).toBeVisible();
    expect(calls(transport.calls, "query_usage_by_app")).toEqual([
      [T - 300 * S, T, "gpu", 200],
    ]);
    await vi.waitFor(() =>
      expect(screen.getByTestId("gpu-range-totals")).toHaveTextContent(
        /Avg · 5 min[\d.]+%Peak · 5 min\d+%/
      )
    );
    // The table reads the engine's ring: no view asks for live GPU rows.
    expect(calls(transport.calls, "set_process_interest")).toEqual([]);
  });

  it("a brushed range scopes the table", async () => {
    const { user } = render();
    await screen.findByRole("table", { name: "GPU by app" });
    act(() =>
      screen.getByRole("slider", { name: "Select a time range" }).focus()
    );
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", { name: "GPU by app, selected 10 s" })
    ).toBeVisible();
  });

  it("without per-process GPU time: the range figures, no apps table", async () => {
    render(["no-process-gpu"]);
    await vi.waitFor(() =>
      expect(screen.getByTestId("gpu-range-totals")).toHaveTextContent(
        /Avg · 5 min[\d.]+%/
      )
    );
    expect(screen.queryByRole("table", { name: "GPU by app" })).toBeNull();
    expect(
      screen.getByRole("slider", { name: "Select a time range" })
    ).toBeVisible();
  });
});
