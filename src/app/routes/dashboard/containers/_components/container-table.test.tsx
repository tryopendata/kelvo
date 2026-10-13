import type { DockerContainer } from "@core/generated/bindings";
import { mockContainer } from "@core/mock/docker";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { expect, it, vi } from "vitest";
import { ContainerTable } from "./container-table";

const defaults = {
  sort: "cpu" as const,
  onSort: () => {},
  grouped: false,
  onCollapse: () => {},
};
const first = {
  ...mockContainer(0, 1000),
  name: "api",
  project: "liveoak",
  lifecycle: "running" as const,
  metrics: { ...mockContainer(0, 1000).metrics, cpu_pct: 200 },
};
const second = {
  ...mockContainer(1, 1000),
  name: "web",
  project: null,
  lifecycle: "running" as const,
  metrics: { ...mockContainer(1, 1000).metrics, cpu_pct: 100 },
};

it("holds row order while focused, updates values, and resumes on blur without a status bar", () => {
  let update: (rows: DockerContainer[]) => void = () => {};
  function Harness() {
    const [rows, setRows] = useState<DockerContainer[]>([first, second]);
    update = setRows;
    return <ContainerTable {...defaults} rows={rows} />;
  }
  const result = renderWithProviders(<Harness />);
  const names = () =>
    within(screen.getByRole("table"))
      .getAllByRole("link")
      .map((node) => node.textContent);
  expect(names()[0]).toContain("api");
  act(() => screen.getByRole("link", { name: /api/ }).focus());
  act(() =>
    update([
      { ...first, metrics: { ...first.metrics, cpu_pct: 10 } },
      { ...second, metrics: { ...second.metrics, cpu_pct: 300 } },
    ])
  );
  expect(names()[0]).toContain("api");
  expect(screen.getByText("300%")).toBeVisible();
  expect(screen.queryByText("Order paused")).toBeNull();
  fireEvent.blur(screen.getByRole("link", { name: /api/ }), {
    relatedTarget: document.body,
  });
  expect(names()[0]).toBe("web");
  result.unmount();
  result.transport.dispose();
});

it("resumes live sorting after a header context click and pointer leave", () => {
  let update: (rows: DockerContainer[]) => void = () => {};
  function Harness() {
    const [rows, setRows] = useState<DockerContainer[]>([first, second]);
    update = setRows;
    return <ContainerTable {...defaults} rows={rows} />;
  }
  const result = renderWithProviders(<Harness />);
  const table = screen.getByRole("table");
  const scroller = table.parentElement;
  if (!scroller) throw new Error("Expected table scroller");
  fireEvent.pointerEnter(scroller);
  fireEvent.contextMenu(
    within(table).getByRole("columnheader", { name: /Name/ })
  );
  act(() =>
    update([
      { ...first, metrics: { ...first.metrics, cpu_pct: 10 } },
      { ...second, metrics: { ...second.metrics, cpu_pct: 300 } },
    ])
  );
  fireEvent.pointerLeave(scroller);
  expect(within(table).getAllByRole("link")[0]).toHaveTextContent("web");
  result.unmount();
  result.transport.dispose();
});

it("separates chart swatches from recording and reports null stopped metrics", () => {
  const result = renderWithProviders(
    <ContainerTable
      {...defaults}
      rows={[
        {
          ...first,
          lifecycle: "exited",
          metrics: { ...first.metrics, cpu_pct: 500 },
        },
      ]}
      slots={{ [first.id]: 2 }}
    />
  );
  expect(screen.queryByRole("checkbox")).toBeNull();
  expect(screen.getByText("1 matching containers")).toBeVisible();
  expect(screen.queryByText("500%")).toBeNull();
  expect(screen.queryByText("No explicit limit")).toBeNull();
  expect(screen.queryByText("No check")).toBeNull();
  expect(
    within(screen.getByRole("table")).getAllByText("—").length
  ).toBeGreaterThanOrEqual(3);
  result.unmount();
  result.transport.dispose();
});

it("shows every unpaged row below the virtualization threshold and links row hover to its chart", () => {
  const focus = vi.fn();
  const rows = Array.from({ length: 40 }, (_, i) => ({
    ...mockContainer(i, 1000),
    name: `container ${i}`,
  }));
  const result = renderWithProviders(
    <ContainerTable
      {...defaults}
      rows={rows}
      slots={{ [rows[0]?.id ?? ""]: 1 }}
      onFocus={focus}
    />
  );
  expect(screen.getAllByRole("link")).toHaveLength(40);
  expect(screen.queryByRole("button", { name: "Next rows" })).toBeNull();
  const row = screen.getByRole("link", { name: /container 0/ }).closest("tr");
  if (row) fireEvent.pointerEnter(row);
  expect(focus).toHaveBeenCalledWith(rows[0]?.id);
  result.unmount();
  result.transport.dispose();
});

it("historical mode keeps removed identities and labels metric peaks", () => {
  const result = renderWithProviders(
    <ContainerTable
      {...defaults}
      rows={[{ ...first, lifecycle: "removed" }]}
      historical
      title="Containers · 11:20–11:21"
    />
  );
  expect(screen.getByRole("columnheader", { name: /Peak CPU/ })).toBeVisible();
  expect(
    screen.getByRole("columnheader", { name: /Peak memory/ })
  ).toBeVisible();
  expect(screen.getByText("200%")).toBeVisible();
  expect(screen.queryByRole("columnheader", { name: "Net" })).toBeNull();
  expect(screen.queryByRole("button", { name: /Start|Stop/ })).toBeNull();
  result.unmount();
  result.transport.dispose();
});
