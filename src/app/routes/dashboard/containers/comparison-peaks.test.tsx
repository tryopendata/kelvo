import { createMockTransport } from "@core/mock-transport";
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it } from "vitest";
import ContainersRoute from "./route";

it.each([
  {
    metric: "cpu",
    ceiling: "6.3%",
    peak: "6%",
    average: "2.3% avg",
    statistics: "1% / 2.3% / 6%",
  },
  {
    metric: "cpu",
    ceiling: "62%",
    peak: "58.6%",
    average: "11.3% avg",
    statistics: "1% / 11.3% / 58.6%",
    cpuAverage: 11.34904,
    cpuPeak: 58.56618,
  },
  {
    metric: "memory",
    ceiling: "180 MB",
    peak: "166 MB",
    average: "110 MB avg",
    statistics: "75.0 MB / 110 MB / 166 MB",
  },
])(
  "shows the $peak $metric spike in the plot, hover, history values and selected table",
  async ({
    metric,
    ceiling,
    peak,
    average,
    statistics,
    cpuAverage = 2.3,
    cpuPeak = 6,
  }) => {
    const end = 1791900000000;
    const transport = createMockTransport({
      scenarios: ["docker-11"],
      historyRows: 1,
      now: () => end,
    });
    const read = transport.dockerComparison;
    transport.dockerComparison = async (request) => {
      const result = await read({
        ...request,
        from_ms: end - 900_000,
        to_ms: end,
      });
      if (result.status !== "ok") return result;
      const series = result.data.series.find(
        (row) => row.container.id === "container-0"
      );
      const template = series?.points[0];
      const ranking = result.data.ranking.find(
        (row) => row.container.id === "container-0"
      );
      if (!series || !template || !ranking)
        throw new Error("Missing Docker fixture");
      // One 10-second bucket with a brief spike well above its average.
      // The API already publishes these distinct facts for both metrics.
      return {
        ...result,
        data: {
          ...result.data,
          coverage: [],
          series: [
            {
              ...series,
              points: [
                {
                  ...template,
                  time_ms: end - 300_000,
                  duration_ms: 10_000,
                  cpu_measured_ms: 10_000,
                  memory_measured_ms: 10_000,
                  cpu_covered: true,
                  memory_covered: true,
                  metrics: {
                    ...template.metrics,
                    cpu_pct: cpuAverage,
                    working_set_bytes: 110_000_000,
                  },
                  peak: {
                    ...template.peak,
                    cpu_pct: cpuPeak,
                    working_set_bytes: 166_000_000,
                  },
                  minimum: {
                    ...template.minimum,
                    cpu_pct: 1,
                    working_set_bytes: 75_000_000,
                  },
                },
              ],
            },
          ],
          ranking: [
            {
              ...ranking,
              peak_cpu_pct: cpuPeak,
              avg_cpu_pct: cpuAverage,
              peak_working_set_bytes: 166_000_000,
              avg_working_set_bytes: 110_000_000,
            },
          ],
        },
      };
    };
    const result = renderWithProviders(<ContainersRoute />, {
      transport,
      route: `/dashboard/containers?metric=${metric}&chart=container-0&end=${end}`,
    });
    await waitFor(() => expect(screen.getByText(ceiling)).toBeVisible());
    expect(screen.getByText(/10 s average · shaded min–max/)).toBeVisible();
    expect(
      screen
        .getByRole("img", { name: /by container,/ })
        .querySelector("path[fill-opacity]")
    ).toBeVisible();
    const brush = screen.getByRole("slider", { name: "Select a time range" });
    brush.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 900, height: 220 }) as DOMRect;
    fireEvent.pointerDown(brush, { button: 0, clientX: 600 });
    fireEvent.pointerUp(brush, { clientX: 600 });
    await waitFor(() =>
      expect(
        within(screen.getByRole("table", { name: /^Containers ·/ })).getByText(
          peak
        )
      ).toBeVisible()
    );
    expect(
      within(screen.getByRole("table", { name: /^Containers ·/ })).getByText(
        average
      )
    ).toBeVisible();
    expect(screen.getByRole("tooltip")).toHaveTextContent(statistics);
    expect(screen.getByRole("tooltip")).toHaveTextContent("min / avg / max");
    await result.user.click(
      screen.getByRole("button", { name: "Show as table" })
    );
    expect(
      within(
        screen.getByRole("table", { name: "Container history values" })
      ).getByText(statistics)
    ).toBeVisible();
    result.unmount();
    transport.dispose();
  }
);
