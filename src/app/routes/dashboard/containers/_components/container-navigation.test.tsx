import { mockContainer, mockHistoryPoints } from "@core/mock/docker";
import { fireEvent, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it } from "vitest";
import { ComparisonChart } from "./comparison-chart";
import { ContainerTable } from "./container-table";

const route =
  "/dashboard/containers?project=liveoak&q=api&chart=container-0&range=1h&end=60000";
const row = { ...mockContainer(0, 60000), name: "api" };

it("captures table return offsets at click time rather than render time", async () => {
  const result = renderWithProviders(
    <main>
      <ContainerTable
        rows={[row]}
        sort="cpu"
        onSort={() => {}}
        grouped={false}
        onCollapse={() => {}}
      />
    </main>,
    { route }
  );
  const main = result.container.querySelector("main");
  const scroller = screen.getByRole("table").parentElement;
  if (!main || !scroller) throw new Error("Missing scroll surfaces");
  main.scrollTop = 217;
  scroller.scrollTop = 143;
  const link = screen.getByRole("link", { name: /api/ });
  for (const modifier of ["ctrlKey", "metaKey", "shiftKey", "altKey"]) {
    fireEvent.click(link, { [modifier]: true });
    expect(
      result.router.state.location.pathname +
        result.router.state.location.search
    ).toBe(route);
  }
  fireEvent.click(link);
  await waitFor(() =>
    expect(result.router.state.location.state).toEqual({
      back: route,
      scroll: 217,
      tableScroll: 143,
    })
  );
  result.unmount();
  result.transport.dispose();
});

for (const via of ["tooltip", "keyboard"] as const) {
  it(`preserves comparison return state through ${via} navigation`, async () => {
    const result = renderWithProviders(
      <main>
        <ComparisonChart
          series={[
            {
              container: row,
              points: mockHistoryPoints(
                {
                  engine_id: row.engine_id,
                  container_id: row.id,
                  from_ms: 0,
                  to_ms: 60000,
                  max_points: 60,
                },
                row
              ),
              slot: 1,
            },
          ]}
          metric="cpu"
          from={0}
          to={60000}
          live={false}
          range="1h"
          focused={row.id}
          pinnedFocus={null}
          onFocus={() => {}}
          onPinFocus={() => {}}
          onChoose={() => {}}
        />
        <div data-container-table-scroll />
      </main>,
      { route }
    );
    const chart = screen.getByRole("img");
    fireEvent.keyDown(chart, { key: "ArrowLeft" });
    const main = result.container.querySelector("main");
    const scroller = result.container.querySelector(
      "[data-container-table-scroll]"
    );
    if (!main || !scroller) throw new Error("Missing scroll surfaces");
    main.scrollTop = 119;
    scroller.scrollTop = 81;
    if (via === "tooltip")
      fireEvent.click(screen.getByRole("link", { name: "api" }));
    else fireEvent.keyDown(chart, { key: "Enter" });
    await waitFor(() =>
      expect(result.router.state.location.state).toEqual({
        back: route,
        scroll: 119,
        tableScroll: 81,
      })
    );
    expect(result.router.state.location.search).toContain("end=");
    result.unmount();
    result.transport.dispose();
  });
}
