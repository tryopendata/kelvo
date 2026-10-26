import type { SeriesStats } from "@core/generated/bindings";
import { createMockTransport } from "@core/mock-transport";
import type { CommandResult } from "@core/transport";
import { act, screen } from "@testing-library/react";
import { useState } from "react";
import { useSeriesStats } from "~/hooks/use-series-stats";
import { renderWithProviders } from "../../../../../../tests/test-utils";
import { LANES } from "../_lib/lanes";
import { laneSummary } from "../_lib/summaries";
import { LaneLabel } from "./lane-label";

const CPU = LANES.find((lane) => lane.id === "cpu");
if (!CPU) throw new Error("CPU lane missing");
const CPU_LANE = CPU;
function Probe() {
  const [fromMs, setFrom] = useState(60_000);
  const query = useSeriesStats(["cpu.total"], {
    fromMs,
    toMs: fromMs + 60_000,
  });
  return (
    <>
      <button type="button" onClick={() => setFrom(180_000)}>
        Next selection
      </button>
      <LaneLabel
        def={CPU_LANE}
        summary={laneSummary(
          "cpu",
          query.data,
          { rate: "MBps", temperature: "C" },
          query.isError
        )}
        onShowTable={() => {}}
      />
    </>
  );
}
const answer = (from: number, value: number): CommandResult<SeriesStats> => ({
  status: "ok",
  data: {
    from_ms: from,
    to_ms: from + 60_000,
    metrics: [
      {
        metric: "cpu.total",
        avg: value,
        max: value + 10,
        integral: value * 60,
        measured_ms: 30_000,
      },
    ],
  },
});

it("shows recorded statistics and coverage, and ignores a slower previous selection", async () => {
  const pending = new Map<
    number,
    (result: CommandResult<SeriesStats>) => void
  >();
  const transport = createMockTransport({ autoTick: false });
  transport.querySeriesStats = (_host, _metrics, from) =>
    new Promise((resolve) => pending.set(from, resolve));
  const { user } = renderWithProviders(<Probe />, { transport });
  await screen.findByText("Loading range summary…");
  await user.click(screen.getByRole("button", { name: "Next selection" }));
  await act(async () => pending.get(180_000)?.(answer(180_000, 60)));
  await screen.findByText("60%");
  expect(screen.getByText("Average · peak 70%")).toBeVisible();
  expect(screen.getByText("50% measured")).toBeVisible();
  await act(async () => pending.get(60_000)?.(answer(60_000, 10)));
  expect(screen.getByText("60%")).toBeVisible();
  expect(screen.queryByText("10%")).not.toBeInTheDocument();
});

it("shows an unavailable historical summary when the transport read fails", async () => {
  const transport = createMockTransport({ autoTick: false });
  transport.querySeriesStats = () =>
    Promise.reject(new Error("history offline"));
  renderWithProviders(<Probe />, { transport });
  await screen.findByText("Summary unavailable");
  expect(screen.getByText("—")).toBeVisible();
});
