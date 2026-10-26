import type { SeriesStats } from "@core/generated/bindings";
import { laneSummary } from "./summaries";

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
