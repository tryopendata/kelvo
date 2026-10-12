import type { DockerContainer } from "@core/generated/bindings";
import { mockContainer } from "@core/mock/docker";
import { act, fireEvent, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { type ComponentProps, type ReactNode, useState } from "react";
import { expect, it, vi } from "vitest";
import {
  ContainerIdentity,
  ContainerMemory,
  ContainerTable,
} from "./container-table";

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

it("memoizes identity and memory cells until their displayed fields change", () => {
  let update: ((row: DockerContainer) => void) | undefined;
  // Spy on the real memo bodies, leaving both comparators and rendering intact.
  const identity = vi.spyOn(
    ContainerIdentity as unknown as {
      type: (props: ComponentProps<typeof ContainerIdentity>) => ReactNode;
    },
    "type"
  );
  const memory = vi.spyOn(
    ContainerMemory as unknown as {
      type: (props: ComponentProps<typeof ContainerMemory>) => ReactNode;
    },
    "type"
  );
  const onMonitored = () => {};
  function Harness() {
    const [row, setRow] = useState(mockContainer(0, 1000));
    update = setRow;
    return (
      <table>
        <tbody>
          <tr>
            <ContainerIdentity
              row={row}
              monitoringFull={false}
              onMonitored={onMonitored}
              back="/"
              scroll={0}
            />
            <ContainerMemory row={row} io={false} />
          </tr>
        </tbody>
      </table>
    );
  }
  const result = renderWithProviders(<Harness />, {
    transportOptions: { historyRows: 1 },
  });
  identity.mockClear();
  memory.mockClear();
  const tick = { ...mockContainer(0, 1000), observed_ms: 2000 };
  act(() => update?.(tick));
  expect(identity).toHaveBeenCalledTimes(0);
  expect(memory).toHaveBeenCalledTimes(0);
  const renamed = { ...tick, name: "Renamed worker" };
  act(() => update?.(renamed));
  expect(screen.getByRole("link", { name: "Renamed worker" })).toBeVisible();
  expect(identity).toHaveBeenCalledTimes(1);
  expect(memory).toHaveBeenCalledTimes(0);
  act(() => update?.({ ...renamed, explicit_limit_bytes: 4_000_000 }));
  expect(screen.getByText("/ 4.0 MB")).toBeVisible();
  expect(identity).toHaveBeenCalledTimes(1);
  expect(memory).toHaveBeenCalledTimes(1);
  identity.mockRestore();
  memory.mockRestore();
  result.unmount();
  result.transport.dispose();
});

it("counts projects across pages and sorts the displayed I/O column", () => {
  const rows = Array.from({ length: 40 }, (_, index) => ({
    ...mockContainer(index, 1000),
    project: "Whole project",
  }));
  const result = renderWithProviders(
    <ContainerTable
      rows={rows}
      sort="cpu"
      onSort={() => {}}
      grouped
      io
      collapsed={[]}
      onCollapse={() => {}}
      onMonitored={() => {}}
      monitoringFull={false}
      page={1}
    />,
    { transportOptions: { historyRows: 1 } }
  );
  expect(
    screen.getByRole("button", { name: /Whole project.*40 containers/ })
  ).toBeVisible();
  expect(screen.getByRole("columnheader", { name: /Network/ })).toHaveAttribute(
    "aria-sort",
    "descending"
  );
  const links = within(screen.getByRole("table")).getAllByRole("link");
  // I/O rows are sorted by their combined displayed receive/send rates.
  const expected = [...rows]
    .sort(
      (a, b) =>
        (b.metrics.network_rx_bps ?? -1) +
          (b.metrics.network_tx_bps ?? -1) -
          ((a.metrics.network_rx_bps ?? -1) +
            (a.metrics.network_tx_bps ?? -1)) || a.id.localeCompare(b.id)
    )
    .slice(32);
  expect(links.map((link) => link.textContent)).toEqual(
    expected.map((row) => row.name)
  );
  result.unmount();
  result.transport.dispose();
});

it("captures the current main scroll position through a scroll event", async () => {
  const main = document.createElement("main");
  document.body.append(main);
  const result = renderWithProviders(
    <ContainerTable
      rows={[mockContainer(0, 1000)]}
      sort="name"
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
  main.scrollTop = 240;
  fireEvent.scroll(main);
  fireEvent.click(screen.getByRole("link", { name: "worker" }));
  expect(result.router.state.location.state).toMatchObject({ scroll: 240 });
  result.unmount();
  result.transport.dispose();
  main.remove();
});
