import { clockTime } from "@core/history-state";
import type { ScenarioName } from "@core/mock/fixtures";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { LiveMirrorChart } from "~/components/live-mirror-chart";
import { CoreLoadCard } from "~/routes/dashboard/cpu/_components/core-load-card";
import { TotalCard } from "~/routes/dashboard/cpu/_components/total-card";
import { useGapBands } from "./use-gap-bands";

const NOW = 1_800_000_000_000;
const MIN = 60_000;
// A sleep inside the last hour (mock `sleep-gap` scenario).
const ASLEEP = `Asleep ${clockTime(NOW - 52 * MIN)}–${clockTime(NOW - 23 * MIN)} · not interpolated`;

function Charts({ windowMs }: { windowMs: number }) {
  const gaps = useGapBands("network");
  return (
    <>
      <TotalCard windowMs={windowMs} />
      <CoreLoadCard clusters={[]} windowMs={windowMs} />
      <LiveMirrorChart
        upKey="net.tx_total"
        downKey="net.rx_total"
        upLabel="Upload"
        downLabel="Download"
        windowMs={windowMs}
        accent="net"
        format={String}
        minCeiling={1}
        gaps={gaps}
      />
    </>
  );
}

async function renderCharts(window: "5m" | "1h", scenarios: ScenarioName[]) {
  const windowMs = window === "1h" ? 60 * MIN : 5 * MIN;
  const r = renderWithProviders(<Charts windowMs={windowMs} />, {
    transportOptions: { now: () => NOW, scenarios, historyRows: 3600 },
    backfillMs: 60 * MIN,
  });
  await screen.findByRole("img", { name: /^CPU total and system/ });
  act(() => r.transport.tick());
  return r;
}

const historyCalls = (r: Awaited<ReturnType<typeof renderCharts>>) =>
  r.transport.calls.filter((c) => c.command === "query_history").length;

describe("gap bands on module page charts (plan 4.17)", () => {
  it("labels a sleep inside the window on the area and the mirrored chart", async () => {
    await renderCharts("1h", ["sleep-gap"]);
    await waitFor(() =>
      expect(screen.getAllByRole("note", { name: ASLEEP })).toHaveLength(2)
    );
  });

  it("draws nothing for a gap outside the window", async () => {
    const r = await renderCharts("5m", ["sleep-gap"]);
    await waitFor(() => expect(historyCalls(r)).toBeGreaterThan(0));
    expect(screen.queryByRole("note")).toBeNull();
  });

  it("asks history for the gaps once for every chart on the page", async () => {
    const r = await renderCharts("1h", ["sleep-gap"]);
    await screen.findAllByRole("note", { name: ASLEEP });
    expect(historyCalls(r)).toBe(1);
  });

  it("marks an open pause up to now", async () => {
    await renderCharts("1h", ["paused"]);
    await waitFor(() =>
      expect(screen.getAllByRole("note", { name: "Paused" })).toHaveLength(2)
    );
  });
});
