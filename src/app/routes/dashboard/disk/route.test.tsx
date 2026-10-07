import { createMockTransport } from "@core/mock-transport";
import { act, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import MemoryRoute from "../memory/route";
import DiskRoute from "./route";

const T = 1_700_000_000_000; // a 10 s edge
const NOW = T + 5000;
const S = 1000;

const calls = (c: { command: string; args: unknown[] }[], command: string) =>
  c.filter((x) => x.command === command).map((x) => x.args.slice(1));

function render(page: React.ReactElement) {
  const transport = createMockTransport({
    now: () => NOW,
    autoTick: false,
    chartWindow: "5m",
  });
  return renderWithProviders(page, { transport, backfillMs: 600_000 });
}

describe("Disk and Memory pages over a range (D-099)", () => {
  it("Disk: bytes per app, ranked by total, and the bytes the disks moved", async () => {
    const { transport, user } = render(<DiskRoute />);
    const table = await screen.findByRole("table", { name: "Disk by app" });
    expect(
      screen.getByRole("heading", { name: "Disk by app, last 5 minutes" })
    ).toBeVisible();
    expect(
      within(table).getByRole("columnheader", { name: /Total/ })
    ).toHaveAttribute("aria-sort", "descending");
    // Every listed app moved bytes.
    for (const cell of within(table).getAllByRole("row").slice(1, 4)) {
      expect(cell).not.toHaveTextContent(/0 B0 B0 B/);
    }
    expect(calls(transport.calls, "query_series_stats")).toContainEqual([
      ["disk.read_total", "disk.write_total"],
      T - 300 * S,
      T,
    ]);
    await vi.waitFor(() =>
      expect(screen.getByTestId("disk-range-totals")).toHaveTextContent(
        /Read · 5 min.+BWritten · 5 min.+B/
      )
    );
    // The process table polled 1 s rows; the usage table reads the ring.
    expect(calls(transport.calls, "set_process_interest")).toEqual([]);

    const brush = screen.getByRole("slider", { name: "Select a time range" });
    act(() => brush.focus());
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", { name: "Disk by app, selected 10 s" })
    ).toBeVisible();
    expect(calls(transport.calls, "query_usage_by_app")).toContainEqual([
      T - 10 * S,
      T,
      "disk",
      200,
    ]);
  });

  it("Memory: apps by peak footprint, with their average while running; either chart brushes", async () => {
    const { user } = render(<MemoryRoute />);
    const table = await screen.findByRole("table", { name: "Memory by app" });
    expect(
      within(table).getByRole("columnheader", { name: /Peak/ })
    ).toHaveAttribute("aria-sort", "descending");
    expect(
      within(table).getByRole("columnheader", { name: "Avg while running" })
    ).toBeVisible();
    // Peaks don't add up: no share column and no remainder rows.
    expect(within(table).queryByRole("columnheader", { name: "Share" })).toBe(
      null
    );
    expect(within(table).queryByText("System and other")).toBeNull();
    await vi.waitFor(() =>
      expect(screen.getByTestId("memory-range-totals")).toHaveTextContent(
        /Peak used · 5 min.+BPeak swap · 5 min.+B/
      )
    );
    const [, swap] = screen.getAllByRole("slider", {
      name: "Select a time range",
    });
    act(() => swap?.focus());
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", {
        name: "Memory by app, selected 10 s",
      })
    ).toBeVisible();
    // Both charts show the one selection.
    expect(screen.getAllByTestId("brush-band")).toHaveLength(2);
  });
});
