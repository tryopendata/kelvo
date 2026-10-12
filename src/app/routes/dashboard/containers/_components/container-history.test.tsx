import type { DockerHistoryPage } from "@core/generated/bindings";
import { mockContainer } from "@core/mock/docker";
import { render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { ContainerHistory } from "./container-history";

it.each([true, false])(
  "renders Rust's covered flag (%s), independent of duration fields",
  (covered) => {
    const page: DockerHistoryPage = {
      generation: 1,
      coverage: [],
      metadata: [],
      persistence: {
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
          measured_ms: covered ? 0 : 10000,
          expected_ms: 10000,
          covered,
          population_revision: 1,
          revision: 1,
          explicit_limit_bytes: null,
          engine_capacity_bytes: null,
        },
      ],
    };
    render(
      <ContainerHistory
        page={page}
        metric="cpu_pct"
        title="Coverage"
        from={0}
        to={10000}
      />
    );
    const path = screen.getByRole("img").querySelector("path");
    expect(Boolean(path?.getAttribute("d"))).toBe(covered);
  }
);
