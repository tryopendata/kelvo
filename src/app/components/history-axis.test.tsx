import { floorTo } from "@core/brush";
import { seriesRange } from "@core/chart-math";
import { formatHistoryTime } from "@core/format";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { TotalCard } from "~/routes/dashboard/cpu/_components/total-card";
import { ChartRangeProvider } from "~/stores/chart-range";
import { historyTicks } from "~/widgets/lib/chart-labels";

it("uses the actual nonaligned grid endpoints for historical ticks and keyboard selection", async () => {
  const from = 1_791_000_012_000;
  const to = from + 60_000;
  const transport = createMockTransport({
    now: () => to + 100_000,
    chartWindow: { absolute: { from_ms: from, to_ms: to } },
  });
  transport.queryHistory = async () => ({
    status: "ok",
    data: {
      tier: "s10",
      bucket_ms: 30_000,
      gaps: [],
      series: [
        {
          key: { metric: "cpu.total", labels: [] },
          hold_ms: 30_000,
          points: [{ t: from + 28_000, min: 12, avg: 12, max: 12 }],
        },
      ],
    },
  });
  const { user } = renderWithProviders(
    <ChartRangeProvider>
      <TotalCard windowMs={60_000} />
    </ChartRangeProvider>,
    { transport }
  );
  await screen.findByRole("slider", { name: "Select a time range" });
  const bounds = seriesRange({
    values: { cpu: [null, 12, null] },
    tEndMs: from + 58_000,
    intervalMs: 30_000,
  });
  const ticks = historyTicks(bounds.fromMs, bounds.toMs, false, 5);
  await waitFor(() =>
    expect(screen.getAllByText(ticks[0] ?? "").length).toBeGreaterThan(0)
  );
  expect(screen.getAllByText(ticks.at(-1) ?? "").length).toBeGreaterThan(0);
  const slider = screen.getByRole("slider", { name: "Select a time range" });
  act(() => slider.focus());
  await user.keyboard("{Enter}");
  expect(slider).toHaveAttribute("aria-valuemax", "5");
  expect(await screen.findByTestId("selection-summary")).toHaveTextContent(
    formatHistoryTime(floorTo(bounds.toMs - 1), true)
  );
});
