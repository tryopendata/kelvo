import type { DockerHistoryPage } from "@core/generated/bindings";
import { mockContainer } from "@core/mock/docker";
import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { ContainerHistory } from "./container-history";

it.each([true, false])(
  "renders Rust's covered flag (%s), independent of duration fields",
  (covered) => {
    const page = history(covered);
    render(
      <ContainerHistory
        page={page}
        metric="cpu_pct"
        title="Coverage"
        from={0}
        to={10000}
      />
    );
    const path = screen.getByRole("img").querySelector("[data-line]");
    expect(Boolean(path?.getAttribute("d"))).toBe(covered);
  }
);

function history(covered: boolean): DockerHistoryPage {
  return {
    generation: 1,
    coverage: [],
    metadata: [],
    persistence: {
      error: null,
      status: "live_only",
      logical_bytes: 0,
      allowance_bytes: 1,
      retained_from_ms: null,
    },
    points: [
      {
        time_ms: 0,
        duration_ms: 10000,
        metrics: mockContainer(0, 0).metrics,
        cpu_measured_ms: covered ? 0 : 10000,
        memory_measured_ms: 0,
        peak: mockContainer(0, 0).metrics,
        minimum: mockContainer(0, 0).metrics,
        expected_ms: 10000,
        cpu_covered: covered,
        memory_covered: false,
        population_revision: 1,
        revision: 1,
        explicit_limit_bytes: null,
        engine_capacity_bytes: null,
      },
    ],
  };
}

it.each([
  { metric: "cpu_pct" as const, half: "0.5%" },
  { metric: "network_rx_bps" as const, half: "0.5 B/s" },
])(
  "labels a quiet $metric axis without rounding its midpoint to its maximum",
  ({ metric, half }) => {
    const page = history(true);
    page.points = page.points.map((point) => ({
      ...point,
      metrics: { ...point.metrics, [metric]: 0 },
    }));
    render(
      <ContainerHistory
        page={page}
        metric={metric}
        title="Quiet container"
        from={0}
        to={10000}
      />
    );
    expect(screen.getByText(half)).toBeVisible();
  }
);
