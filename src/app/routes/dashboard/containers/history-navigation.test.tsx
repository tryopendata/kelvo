import { createMockTransport } from "@core/mock-transport";
import { waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Route, Routes } from "react-router";
import { expect, it, vi } from "vitest";
import ContainerDetailRoute from "./detail-route";
import ContainersRoute from "./route";

it.each([
  {
    view: "activity",
    path: "/dashboard/containers?range=constructor&end=Infinity",
    duration: 900_000,
  },
  {
    view: "detail",
    path: "/dashboard/containers/mock-orbstack/container-0?range=toString&at=NaN",
    duration: 900_000,
  },
  {
    view: "retained",
    path: "/dashboard/containers?retained=1&retainedRange=__proto__&retainedEnd=Infinity",
    duration: 3_600_000,
  },
])(
  "normalizes invalid $view navigation before requesting history",
  async ({ view, path, duration }) => {
    const transport = createMockTransport({
      scenarios: ["docker-11"],
      historyRows: 1,
    });
    const comparison = vi.spyOn(transport, "dockerComparison");
    const history = vi.spyOn(transport, "dockerHistory");
    const result = renderWithProviders(
      <Routes>
        <Route path="/dashboard/containers" element={<ContainersRoute />} />
        <Route
          path="/dashboard/containers/:engineId/:containerId"
          element={<ContainerDetailRoute />}
        />
      </Routes>,
      { transport, route: path }
    );
    try {
      await waitFor(() => {
        const requests =
          view === "activity" ? comparison.mock.calls : history.mock.calls;
        expect(requests.length).toBeGreaterThan(0);
        for (const [request] of requests) {
          expect(Number.isFinite(request.to_ms)).toBe(true);
          expect(request.to_ms - request.from_ms).toBe(duration);
        }
      });
    } finally {
      result.unmount();
      transport.dispose();
    }
  }
);
