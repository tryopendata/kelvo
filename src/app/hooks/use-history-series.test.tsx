import { createMockTransport } from "@core/mock-transport";
import { historyKeys } from "@core/query-keys";
import { act, waitFor } from "@testing-library/react";
import { FrequencyCard } from "~/routes/dashboard/gpu/_components/frequency-card";
import { ChartRangeProvider } from "~/stores/chart-range";
import { renderWithProviders } from "../../../tests/test-utils";
import { useChartWindow } from "./use-chart-window";
import { useModuleSeries } from "./use-module-series";
import { useRangeScope } from "./use-range-scope";
import type { WindowSeries } from "./use-window-series";

const NOW = 1_791_000_000_000;
const HOUR = 3_600_000;

it.each([6 * HOUR, 90 * 24 * HOUR])(
  "keeps one-point GPU residency queries current over %i ms",
  async (windowMs) => {
    const now = NOW + 123_456;
    const { transport } = renderWithProviders(
      <ChartRangeProvider>
        <FrequencyCard windowMs={windowMs} />
      </ChartRangeProvider>,
      {
        transportOptions: {
          now: () => now,
          chartWindow: { relative: windowMs },
        },
      }
    );
    await waitFor(() => {
      const request = transport.calls
        .filter((c) => c.command === "query_history")
        .map(
          (c) =>
            c.args[0] as {
              from_ms: number;
              to_ms: number;
              max_points: number;
              selectors: { metric: string }[];
            }
        )
        .find(
          (r) =>
            r.max_points === 1 &&
            r.selectors.some((s) => s.metric === "gpu.residency")
        );
      expect(request).toBeDefined();
      expect(now - (request?.to_ms ?? 0)).toBeLessThan(60_000);
      expect((request?.to_ms ?? 0) - (request?.from_ms ?? 0)).toBe(windowMs);
    });
  }
);

it("does not hold a fixed page under a rolling range while its answer is pending", async () => {
  const end = NOW - 24 * HOUR;
  const transport = createMockTransport({
    now: () => NOW,
    chartWindow: { absolute: { from_ms: end - 6 * HOUR, to_ms: end } },
  });
  const read = transport.queryHistory.bind(transport);
  let release: (() => void) | undefined;
  transport.queryHistory = async (request) => {
    if (request.to_ms > end)
      await new Promise<void>((resolve) => {
        release = resolve;
      });
    return read(request);
  };
  renderWithProviders(
    <ChartRangeProvider>
      <Probe />
    </ChartRangeProvider>,
    { transport }
  );
  await waitFor(() =>
    expect(latest.values["cpu.total"]?.length).toBeGreaterThan(0)
  );
  try {
    await act(() =>
      transport.updateSettings({
        general: { chart_window: { relative: 6 * HOUR } },
      })
    );
    await waitFor(() => expect(release).toBeDefined());
    expect(latest.loading).toBe(true);
    expect(latest.values).toEqual({});
  } finally {
    await act(async () => release?.());
  }
  await waitFor(() => expect(latest.loading).toBe(false));
});
let latest: WindowSeries;
let scope: ReturnType<typeof useRangeScope>;
let renders = 0;

it.each(["typed", "rejected"])(
  "surfaces a %s history failure and recovers on retry",
  async (failure) => {
    const transport = createMockTransport({
      now: () => NOW,
      chartWindow: { relative: 6 * HOUR },
    });
    const read = transport.queryHistory.bind(transport);
    let fail = true;
    transport.queryHistory = async (request) => {
      if (!fail) return read(request);
      if (failure === "rejected") throw new Error("channel closed");
      return {
        status: "error",
        error: { kind: "remote_unreachable", message: "disconnected" },
      };
    };
    const { queryClient } = renderWithProviders(
      <ChartRangeProvider>
        <Probe />
      </ChartRangeProvider>,
      { transport }
    );
    await waitFor(() => expect(latest.error).toBe(true));
    expect(latest.loading).toBe(false);
    expect(latest.values).toEqual({});
    fail = false;
    await act(() =>
      queryClient.invalidateQueries({ queryKey: historyKeys.all })
    );
    await waitFor(() => expect(latest.error).toBe(false));
    expect(latest.values["cpu.total"]?.some((v) => v !== null)).toBe(true);
  }
);
function Probe() {
  renders++;
  const windowMs = useChartWindow()?.windowMs ?? 300_000;
  latest = useModuleSeries(["cpu.total"], windowMs);
  scope = useRangeScope(windowMs);
  return null;
}

it("reads a bounded six-hour history, including samples older than the live ring", async () => {
  const { transport } = renderWithProviders(
    <ChartRangeProvider>
      <Probe />
    </ChartRangeProvider>,
    {
      transportOptions: { now: () => NOW, chartWindow: { relative: 6 * HOUR } },
    }
  );
  await waitFor(() =>
    expect(latest.values["cpu.total"]?.some((value) => value !== null)).toBe(
      true
    )
  );
  await waitFor(() => expect(latest.intervalMs).toBeGreaterThan(1000));
  const values = latest.values["cpu.total"] ?? [];
  expect(values.length).toBeLessThanOrEqual(1201);
  expect(
    values
      .slice(0, Math.floor(values.length / 2))
      .some((value) => value !== null)
  ).toBe(true);
  const request = transport.calls.find(
    (call) =>
      call.command === "query_history" &&
      (call.args[0] as { max_points: number }).max_points === 1200
  )?.args[0];
  expect(request).toMatchObject({
    from_ms: NOW - 6 * HOUR,
    to_ms: NOW,
    tier: "auto",
    max_points: 1200,
  });
});

it("keeps fixed chart samples and summary bounds fixed when new live frames arrive", async () => {
  const end = NOW - 2 * HOUR;
  let clock = NOW;
  const { transport } = renderWithProviders(
    <ChartRangeProvider>
      <Probe />
    </ChartRangeProvider>,
    {
      transportOptions: {
        now: () => clock,
        chartWindow: { absolute: { from_ms: end - HOUR, to_ms: end } },
      },
    }
  );
  await waitFor(() => expect(latest.loading).toBe(false));
  expect(latest.values["cpu.total"]?.length).toBeGreaterThan(0);
  expect(scope.range).toEqual({ fromMs: end - HOUR, toMs: end });
  const before = latest;
  const beforeRenders = renders;
  const reads = transport.calls.filter(
    (call) => call.command === "query_history"
  ).length;
  clock += 120_000;
  await act(() => transport.tick());
  expect(renders).toBe(beforeRenders);
  expect(latest).toEqual(before);
  expect(scope.range).toEqual({ fromMs: end - HOUR, toMs: end });
  expect(
    transport.calls.filter((call) => call.command === "query_history")
  ).toHaveLength(reads);
  await act(() =>
    transport.updateSettings({ general: { chart_window: "5m" } })
  );
  await waitFor(() => expect(latest.intervalMs).toBe(1000));
  expect(scope.range?.toMs).toBe(NOW + 120_000);
});
