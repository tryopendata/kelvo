import { createMockTransport } from "@core/mock-transport";
import { act, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { ChartRangeProvider } from "~/stores/chart-range";
import { useChartWindow } from "./use-chart-window";
import { useRangeScope } from "./use-range-scope";
import { useSeriesStats } from "./use-series-stats";

const NOW = 1_791_000_000_000;
const HOUR = 3_600_000;
let summary: ReturnType<typeof useSeriesStats>;
function Probe() {
  const duration = useChartWindow()?.windowMs ?? HOUR;
  const scope = useRangeScope(duration);
  summary = useSeriesStats(["cpu.total"], scope.range, {
    keepPrevious: scope.keepPrevious,
  });
  return null;
}

it("does not label a fixed summary as the new rolling interval while loading", async () => {
  const end = NOW - 24 * HOUR;
  const transport = createMockTransport({
    now: () => NOW,
    chartWindow: { absolute: { from_ms: end - 6 * HOUR, to_ms: end } },
  });
  let release: (() => void) | undefined;
  transport.querySeriesStats = async (_host, _metrics, from, to) => {
    if (to > end)
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    const avg = to > end ? 85 : 11;
    return {
      status: "ok",
      data: {
        from_ms: from,
        to_ms: to,
        metrics: [
          {
            metric: "cpu.total",
            avg,
            max: avg,
            integral: 0,
            measured_ms: to - from,
          },
        ],
      },
    };
  };
  renderWithProviders(
    <ChartRangeProvider>
      <Probe />
    </ChartRangeProvider>,
    { transport }
  );
  await waitFor(() => expect(summary.data?.metrics[0]?.avg).toBe(11));
  try {
    await act(() =>
      transport.updateSettings({
        general: { chart_window: { relative: 6 * HOUR } },
      })
    );
    await waitFor(() => expect(release).toBeDefined());
    expect(summary.isPending).toBe(true);
    expect(summary.data).toBeUndefined();
  } finally {
    await act(async () => release?.());
  }
  await waitFor(() => expect(summary.data?.metrics[0]?.avg).toBe(85));
});
