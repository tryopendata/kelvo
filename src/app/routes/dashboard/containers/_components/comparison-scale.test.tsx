import type { DockerHistoryPoint } from "@core/generated/bindings";
import { mockContainer, mockHistoryPoints } from "@core/mock/docker";
import { screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { expect, it } from "vitest";
import { ComparisonChart } from "./comparison-chart";

const container = mockContainer(0, 60_000);
const template = mockHistoryPoints(
  {
    engine_id: container.engine_id,
    container_id: container.id,
    from_ms: 0,
    to_ms: 60_000,
    max_points: 60,
  },
  container
)[0];
function point(time: number, cpu: number): DockerHistoryPoint {
  if (!template) throw new Error("Expected a recorded Docker fixture point");
  return {
    ...template,
    time_ms: time,
    duration_ms: 10_000,
    cpu_measured_ms: 10_000,
    cpu_covered: true,
    metrics: { ...template.metrics, cpu_pct: cpu },
  };
}
function Chart({
  points,
  metric = "cpu",
}: {
  points: DockerHistoryPoint[];
  metric?: "cpu" | "memory";
}) {
  return (
    <ComparisonChart
      series={[{ container, points, slot: 1 }]}
      metric={metric}
      from={10_000}
      to={60_000}
      live={false}
      range="15m"
      focused={null}
      pinnedFocus={null}
      onFocus={() => {}}
      onPinFocus={() => {}}
      onChoose={() => {}}
    />
  );
}

it.each([
  { values: [1.2, 1.7], top: "2%", middle: "1%" },
  { values: [0.12, 0.17], top: "0.2%", middle: "0.1%" },
  { values: [120, 170], top: "200%", middle: "100%" },
])(
  "fits the labelled CPU axis to visible values $values",
  ({ values, top, middle }) => {
    const result = renderWithProviders(
      <Chart
        points={values.map((value, i) => point(10_000 + i * 10_000, value))}
      />
    );
    expect(screen.getByText(top)).toBeVisible();
    expect(screen.getByText(middle)).toBeVisible();
    result.unmount();
    result.transport.dispose();
  }
);

it("excludes offscreen and unmeasured spikes from the displayed axis", () => {
  const result = renderWithProviders(
    <Chart
      points={[
        point(0, 59),
        point(10_000, 1.7),
        { ...point(20_000, 90), cpu_measured_ms: 0 },
        point(60_000, 120),
      ]}
    />
  );
  expect(screen.getByText("2%")).toBeVisible();
  result.unmount();
  result.transport.dispose();
});

it("refits a paused chart immediately after selecting a lower-load workload", async () => {
  function Selection() {
    const [low, setLow] = useState(false);
    return (
      <>
        <button type="button" onClick={() => setLow(true)}>
          Low-load workload
        </button>
        <Chart points={[point(10_000, low ? 1.7 : 59)]} />
      </>
    );
  }
  const result = renderWithProviders(<Selection />);
  expect(screen.getByText("60%")).toBeVisible();
  await result.user.click(
    screen.getByRole("button", { name: "Low-load workload" })
  );
  expect(screen.getByText("2%")).toBeVisible();
  expect(screen.queryByText("60%")).not.toBeInTheDocument();
  result.unmount();
  result.transport.dispose();
});

it("fits memory to measured working sets instead of a fixed 64 MiB minimum", () => {
  const sample = point(10_000, 0);
  const result = renderWithProviders(
    <Chart
      metric="memory"
      points={[
        {
          ...sample,
          memory_measured_ms: 10_000,
          memory_covered: true,
          metrics: { ...sample.metrics, working_set_bytes: 5_000_000 },
        },
      ]}
    />
  );
  expect(screen.getByText("5.0 MB")).toBeVisible();
  expect(screen.getByText("2.5 MB")).toBeVisible();
  result.unmount();
  result.transport.dispose();
});

it("includes the plotted rollup peak rather than clipping it to the average", () => {
  const sample = point(10_000, 1.7);
  const result = renderWithProviders(
    <Chart
      points={[
        {
          ...sample,
          duration_ms: 60_000,
          cpu_measured_ms: 60_000,
          peak: { ...sample.peak, cpu_pct: 59 },
        },
      ]}
    />
  );
  expect(screen.getByText("60%")).toBeVisible();
  result.unmount();
  result.transport.dispose();
});
