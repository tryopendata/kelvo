import type { DockerContainer } from "@core/generated/bindings";
import { mockContainer } from "@core/mock/docker";
import { act, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { expect, it } from "vitest";
import { ContainerTable } from "./container-table";

it("holds identity while focused as values update, then explicitly reorders", async () => {
  const first = mockContainer(0, 1000);
  const second = mockContainer(1, 1000);
  let update: ((rows: DockerContainer[]) => void) | undefined;
  function Harness() {
    const [rows, setRows] = useState([first, second]);
    update = setRows;
    return (
      <ContainerTable
        rows={rows}
        sort="cpu"
        onSort={() => {}}
        grouped={false}
        io={false}
        collapsed={[]}
        onCollapse={() => {}}
        onMonitored={() => {}}
        monitoringFull={false}
      />
    );
  }
  const result = renderWithProviders(<Harness />, {
    transportOptions: { historyRows: 1 },
  });
  const links = () => within(screen.getByRole("table")).getAllByRole("link");
  expect(links()[0]).toHaveTextContent("worker");
  act(() => screen.getByRole("link", { name: "worker" }).focus());
  act(() =>
    update?.([
      { ...first, metrics: { ...first.metrics, cpu_pct: 10 } },
      { ...second, metrics: { ...second.metrics, cpu_pct: 300 } },
    ])
  );
  expect(links()[0]).toHaveTextContent("worker");
  expect(screen.getByText("300%")).toBeVisible();
  expect(screen.getByRole("link", { name: "worker" })).toHaveFocus();
  await result.user.click(screen.getByRole("button", { name: "Reorder now" }));
  expect(links()[0]).toHaveTextContent("web");
  result.unmount();
  result.transport.dispose();
});

it("does not turn uninspected metadata into an unlimited allowance", () => {
  const row = {
    ...mockContainer(1, 1000),
    metadata_available: false,
    restart_count: null,
  };
  const result = renderWithProviders(
    <ContainerTable
      rows={[row]}
      sort="cpu"
      onSort={() => {}}
      grouped={false}
      io={false}
      collapsed={[]}
      onCollapse={() => {}}
      onMonitored={() => {}}
      monitoringFull={false}
    />,
    { transportOptions: { historyRows: 1 } }
  );
  expect(screen.getByText("Limit unavailable")).toBeVisible();
  expect(screen.queryByText("No explicit limit")).toBeNull();
  const cells = within(screen.getByRole("table")).getAllByRole("cell");
  expect(cells.at(-1)).toHaveTextContent("—");
  result.unmount();
  result.transport.dispose();
});

it("keeps selection controls current after metric ticks and population changes", async () => {
  const first = { ...mockContainer(0, 1000), monitored: true };
  const second = { ...mockContainer(1, 1000), monitored: false };
  let tick: (() => void) | undefined;
  const changes: Array<{ id: string; checked: boolean; revision: number }> = [];
  function Harness() {
    const [rows, setRows] = useState([first, second]);
    const [revision, setRevision] = useState(0);
    tick = () => {
      setRevision((value) => value + 1);
      setRows((previous) =>
        previous.map((row) => ({
          ...row,
          metrics: { ...row.metrics, cpu_pct: 321 },
        }))
      );
    };
    return (
      <ContainerTable
        rows={rows}
        sort="name"
        onSort={() => {}}
        grouped={false}
        io={false}
        collapsed={[]}
        onCollapse={() => {}}
        onMonitored={(id, checked) => {
          changes.push({ id, checked, revision });
          setRows((previous) =>
            previous.map((row) =>
              row.id === id ? { ...row, monitored: checked } : row
            )
          );
        }}
        monitoringFull={rows.some((row) => row.monitored)}
      />
    );
  }
  const result = renderWithProviders(<Harness />, {
    transportOptions: { historyRows: 1 },
  });
  expect(
    screen.getByRole("checkbox", { name: "Monitor worker" })
  ).toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Monitor web" })).toBeDisabled();
  act(() => tick?.());
  expect(screen.getAllByText("321%")).toHaveLength(2);
  await result.user.click(
    screen.getByRole("checkbox", { name: "Monitor worker" })
  );
  expect(changes).toEqual([{ id: first.id, checked: false, revision: 1 }]);
  expect(
    screen.getByRole("checkbox", { name: "Monitor worker" })
  ).not.toBeChecked();
  expect(screen.getByRole("checkbox", { name: "Monitor web" })).toBeEnabled();
  await result.user.click(
    screen.getByRole("checkbox", { name: "Monitor web" })
  );
  expect(changes.at(-1)).toEqual({ id: second.id, checked: true, revision: 1 });
  expect(
    screen.getByRole("checkbox", { name: "Monitor worker" })
  ).toBeDisabled();
  result.unmount();
  result.transport.dispose();
});
