import type { SeriesStats } from "@core/generated/bindings";
import { laneComparison, laneSummary } from "./summaries";

const units = { rate: "MBps", temperature: "C" } as const;
const network = (rx: number | null, tx: number | null): SeriesStats => ({
  from_ms: 0,
  to_ms: 60_000,
  metrics: [
    {
      metric: "net.rx_total",
      avg: null,
      max: null,
      integral: rx,
      measured_ms: 60_000,
    },
    {
      metric: "net.tx_total",
      avg: null,
      max: null,
      integral: tx,
      measured_ms: 30_000,
    },
  ],
});

it("shows recorded RX plus TX totals and separate directional coverage", () => {
  const summary = laneSummary("network", network(2e6, 0.5e6), units);
  expect(summary.value).toBe("2.5 MB");
  expect(summary.coverage).toBe("RX 100% measured · TX 50% measured");
});
it.each([
  [null, 1e6],
  [1e6, null],
])("does not replace an unmeasured network direction with zero", (rx, tx) => {
  expect(laneSummary("network", network(rx, tx), units).value).toBe("—");
});

it("compares authoritative totals without treating missing prior coverage as zero", () => {
  const result = laneComparison(
    "network",
    network(2e6, 0.5e6),
    network(1e6, 0.5e6),
    units
  );
  expect(result.previous).toBe("1.5 MB");
  expect(result.delta).toBe("+1.0 MB");
  expect(result.coverage).toBe("RX 100% measured · TX 50% measured");
  expect(
    laneComparison("network", network(2e6, 0.5e6), network(null, 0.5e6), units)
      .delta
  ).toBe("Difference unavailable");
});

it("converts a temperature difference without adding the Fahrenheit offset", () => {
  const stats = (max: number): SeriesStats => ({
    from_ms: 0,
    to_ms: 60_000,
    metrics: [
      {
        metric: "thermal.hottest",
        avg: max,
        max,
        integral: null,
        measured_ms: 60_000,
      },
    ],
  });
  expect(
    laneComparison("temp", stats(30), stats(20), { ...units, temperature: "F" })
      .delta
  ).toBe("+18.0 °F");
});
