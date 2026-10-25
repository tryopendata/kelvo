import { createMockTransport } from "@core/mock-transport";
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Profiler } from "react";
import { expect, it, vi } from "vitest";
import ContainersRoute from "./route";

// Observe commits to the real subtrees; transport and component behavior stay real.
const renders = vi.hoisted(() => ({ lines: 0, legend: 0 }));
vi.mock("./_components/comparison-lines", async (importOriginal) => {
  const real =
    await importOriginal<typeof import("./_components/comparison-lines")>();
  return {
    ...real,
    ComparisonLines: (props: Parameters<typeof real.ComparisonLines>[0]) => (
      <Profiler id="comparison-lines" onRender={() => renders.lines++}>
        <real.ComparisonLines {...props} />
      </Profiler>
    ),
  };
});
vi.mock("~/widgets/legend", async (importOriginal) => {
  const real = await importOriginal<typeof import("~/widgets/legend")>();
  return {
    ...real,
    Legend: (props: Parameters<typeof real.Legend>[0]) => (
      <Profiler id="comparison-legend" onRender={() => renders.legend++}>
        <real.Legend {...props} />
      </Profiler>
    ),
  };
});

it("keeps unchanged history idle between buckets and updates data and focus", async () => {
  let now = 1_791_900_000_000;
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
    now: () => now,
  });
  const result = renderWithProviders(<ContainersRoute />, { transport });
  try {
    await screen.findByRole("button", { name: "api" });
    await waitFor(() => expect(result.queryClient.isFetching()).toBe(0));
    expect(renders.lines).toBeGreaterThan(0);
    expect(renders.legend).toBeGreaterThan(0);
    const beforeTable = screen.getByRole("table", {
      name: "Containers now",
    }).textContent;
    renders.lines = 0;
    renders.legend = 0;
    now += 1000;
    await act(async () => transport.tick());
    expect(
      screen.getByRole("table", { name: "Containers now" }).textContent
    ).not.toBe(beforeTable);
    expect(renders.lines).toBe(0);
    expect(renders.legend).toBe(0);

    now += 10_000;
    await act(async () => transport.tick());
    await waitFor(() => expect(renders.lines).toBeGreaterThan(0));
    expect(renders.legend).toBeGreaterThan(0);
    fireEvent.pointerEnter(screen.getByRole("button", { name: "api" }));
    expect(
      screen
        .getByRole("img", { name: /^CPU by container/ })
        .querySelector('g[opacity="0.3"]')
    ).not.toBeNull();
    await result.user.click(screen.getByRole("button", { name: "api" }));
    expect(screen.getByRole("button", { name: "api" })).toHaveAttribute(
      "aria-pressed",
      "true"
    );
    await result.user.click(screen.getByRole("radio", { name: "Memory" }));
    expect(
      await screen.findByRole("img", { name: /^Memory by container/ })
    ).toBeVisible();
  } finally {
    result.unmount();
    transport.dispose();
  }
});
