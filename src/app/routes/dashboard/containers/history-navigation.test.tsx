import { createMockTransport } from "@core/mock-transport";
import { waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Route, Routes } from "react-router";
import { expect, it, vi } from "vitest";
import { useSharedContainerRange } from "./_hooks/use-shared-container-range";
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

it("applies a retained link once ahead of legacy chart dates and preserves other filters", async () => {
  const { RangeNavigationProvider } = await import(
    "~/hooks/use-range-navigation"
  );
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const end = 1_791_900_000_000;
  const result = renderWithProviders(
    <RangeNavigationProvider>
      <ContainersRoute />
    </RangeNavigationProvider>,
    {
      transport,
      route: `/dashboard/containers?retained=1&range=24h&end=${end - 86_400_000}&retainedRange=1h&retainedEnd=${end}&q=api&project=work`,
    }
  );
  try {
    await waitFor(() =>
      expect(result.router.state.location.search).not.toContain("retainedRange")
    );
    expect(
      (await transport.getSettings()).settings.general.chart_window
    ).toEqual({ absolute: { from_ms: end - 3_600_000, to_ms: end } });
    const params = new URLSearchParams(result.router.state.location.search);
    expect(params.get("q")).toBe("api");
    expect(params.get("project")).toBe("work");
    expect(params.has("range")).toBe(false);
    expect(params.has("end")).toBe(false);
    expect(params.has("retainedEnd")).toBe(false);
    expect(
      transport.calls.filter((call) => call.command === "update_settings")
    ).toHaveLength(1);
  } finally {
    result.unmount();
    transport.dispose();
  }
});

function MainLinkProbe() {
  const range = useSharedContainerRange(1_791_900_000_000);
  return (
    <p>
      Recorded range: {range.fromMs} to {range.toMs}
    </p>
  );
}

it("honors retained-link precedence even before the retained browser mounts", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const end = 1_791_900_000_000;
  const result = renderWithProviders(<MainLinkProbe />, {
    transport,
    route: `/dashboard/containers?range=24h&end=${end - 86_400_000}&retainedRange=1h&retainedEnd=${end}&q=api`,
  });
  try {
    await waitFor(() =>
      expect(result.router.state.location.search).not.toContain("retainedRange")
    );
    expect(
      (await transport.getSettings()).settings.general.chart_window
    ).toEqual({ absolute: { from_ms: end - 3_600_000, to_ms: end } });
    expect(result.router.state.location.search).toBe("?q=api");
  } finally {
    result.unmount();
    transport.dispose();
  }
});
