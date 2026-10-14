import {
  mockContainer,
  mockDockerSnapshot,
  mockHistoryPoints,
} from "@core/mock/docker";
import { fireEvent, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it, vi } from "vitest";
import { ChartChooser } from "./chart-chooser";
import { ComparisonChart } from "./comparison-chart";
import { mergeCoverage } from "./history-details";
import { HistoryNotice } from "./history-notice";
import { MonitoringDialog } from "./monitoring-dialog";

it("blocks a seventh chart choice and never changes recording", async () => {
  const candidates = Array.from({ length: 7 }, (_, i) => ({
    ...mockContainer(i, 1000),
    name: `container ${i}`,
  }));
  const change = vi.fn();
  const result = renderWithProviders(
    <ChartChooser
      open
      onOpenChange={() => {}}
      candidates={candidates}
      liveRows={candidates}
      selected={candidates.slice(0, 6).map((row) => row.id)}
      slots={{}}
      metric="cpu"
      onChange={change}
      onAutomatic={() => {}}
    />
  );
  expect(
    screen.getByRole("checkbox", { name: "Chart container 6" })
  ).toBeDisabled();
  expect(
    screen.getByText("Remove a container to compare another.")
  ).toBeVisible();
  await result.user.click(
    screen.getByRole("checkbox", { name: "Chart container 0" })
  );
  expect(change).toHaveBeenCalledWith(
    candidates.slice(1, 6).map((row) => row.id)
  );
  result.unmount();
  result.transport.dispose();
});

it("merges touching equal-scope coverage without merging events into metrics", () => {
  const records = mergeCoverage([
    { start_ms: 0, end_ms: 10000, container_id: "a", reason: "write_failed" },
    {
      start_ms: 10000,
      end_ms: 20000,
      container_id: "a",
      reason: "write_failed",
    },
    {
      start_ms: 0,
      end_ms: 20000,
      container_id: null,
      reason: "events_disconnected",
    },
    {
      start_ms: 10000,
      end_ms: 20000,
      container_id: "b",
      reason: "write_failed",
    },
  ]);
  expect(records).toHaveLength(3);
  expect(records.find((r) => r.container_id === "a")?.end_ms).toBe(20000);
});

it("shows one prioritized notice and separates metrics from events in details", async () => {
  const snapshot = mockDockerSnapshot(1, 30000);
  const result = renderWithProviders(
    <HistoryNotice
      snapshot={{
        ...snapshot,
        status: "unavailable",
        persistence: {
          ...snapshot.persistence,
          status: "write_failed",
          error: "Database is locked; close the other writer and retry.",
        },
      }}
      coverage={[
        {
          start_ms: 0,
          end_ms: 10000,
          container_id: null,
          reason: "events_disconnected",
        },
      ]}
      metric="cpu"
      onManage={() => {}}
      onResume={() => {}}
    />
  );
  expect(screen.getAllByRole("status")).toHaveLength(1);
  expect(
    screen.getByText("OrbStack unavailable; recent history not saved")
  ).toBeVisible();
  await result.user.click(screen.getByRole("button", { name: /Details/ }));
  const dialog = screen.getByRole("dialog", { name: "History details" });
  expect(
    within(dialog).getByText("Measurements could not be saved")
  ).toBeVisible();
  expect(
    within(dialog).getByText(
      "Database is locked; close the other writer and retry."
    )
  ).toBeVisible();
  expect(
    within(dialog).getByText("Lost connection to Docker events")
  ).toBeVisible();
  result.unmount();
  result.transport.dispose();
});

it("explains automatic engine inventory collection and saves display preferences immediately", async () => {
  const snapshot = mockDockerSnapshot(11, 30000);
  const configure = vi.fn(async () => true);
  const result = renderWithProviders(
    <MonitoringDialog
      snapshot={snapshot}
      onChange={() => {}}
      onClose={() => {}}
      configure={configure}
    />
  );
  expect(screen.queryByRole("radio", { name: "Choose containers" })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: /^Monitor / })).toBeNull();
  expect(screen.queryByRole("button", { name: "Apply" })).toBeNull();
  expect(
    screen.getByRole("heading", { name: "Automatic collection" })
  ).toBeVisible();
  expect(screen.getByText(/up to 128 at a time/)).toBeVisible();
  expect(configure).not.toHaveBeenCalled();
  await result.user.click(
    screen.getByRole("checkbox", { name: "Show in popover" })
  );
  expect(configure).toHaveBeenCalledWith({ show_popover: true });
  result.unmount();
  result.transport.dispose();
});

it("provides partial values, interval min/avg/max, keyboard focus and pinned detail links", async () => {
  const container = mockContainer(0, 86_400_000);
  const points = mockHistoryPoints(
    {
      engine_id: container.engine_id,
      container_id: container.id,
      from_ms: 0,
      to_ms: 86_400_000,
      max_points: 360,
    },
    container
  );
  const focus = vi.fn();
  const result = renderWithProviders(
    <ComparisonChart
      series={[{ container, points, slot: 1 }]}
      metric="cpu"
      from={0}
      to={86_400_000}
      live={false}
      range="24h"
      focused={null}
      pinnedFocus={null}
      onFocus={focus}
      onPinFocus={() => {}}
      onChoose={() => {}}
    />
  );
  const chart = screen.getByRole("img", { name: /CPU by container/ });
  expect(chart.querySelectorAll("ellipse").length).toBeGreaterThan(0);
  const brush = screen.getByRole("slider", { name: "Select a time range" });
  fireEvent.keyDown(brush, { key: "ArrowLeft" });
  fireEvent.keyDown(brush, { key: "Enter" });
  expect(screen.getByRole("tooltip")).toHaveTextContent("min / avg / max");
  const link = within(screen.getByRole("tooltip")).getByRole("link");
  expect(link.getAttribute("href")).toContain("metric=cpu&range=15m&end=");
  fireEvent.focus(screen.getByRole("button", { name: container.name }));
  expect(focus).toHaveBeenCalledWith(container.id);
  await result.user.click(
    screen.getByRole("button", { name: "Show as table" })
  );
  expect(
    screen.getByRole("table", { name: "Container history values" })
  ).toHaveTextContent("partial");
  result.unmount();
  result.transport.dispose();
});

it("keeps the unavailable last-reading time at the newest measured container observation", () => {
  const snapshot = mockDockerSnapshot(2, 90_000);
  const measured = 20_000;
  const result = renderWithProviders(
    <HistoryNotice
      snapshot={{
        ...snapshot,
        status: "unavailable",
        observed_ms: 90_000,
        containers: snapshot.containers.map((row, index) => ({
          ...row,
          observed_ms: index ? measured : 10_000,
        })),
      }}
      coverage={[]}
      metric="cpu"
      onManage={() => {}}
      onResume={() => {}}
    />
  );
  expect(screen.getByRole("status")).toHaveTextContent(
    `last reading ${new Date(measured).toLocaleTimeString()}`
  );
  result.unmount();
  result.transport.dispose();
});

it.each([0, 3])(
  "reports %s active collectors against the current running population",
  (expected) => {
    const snapshot = mockDockerSnapshot(11, 30_000);
    const result = renderWithProviders(
      <HistoryNotice
        snapshot={{
          ...snapshot,
          aggregate: {
            ...snapshot.aggregate,
            monitored: 9,
            expected,
            running: 5,
            partial: true,
          },
        }}
        coverage={[]}
        metric="cpu"
        onManage={() => {}}
        onResume={() => {}}
      />
    );
    expect(screen.getByRole("status")).toHaveTextContent(
      `Monitoring ${expected} of 5 running containers`
    );
    expect(
      screen.queryByText("Monitoring 9 of 5 running containers")
    ).toBeNull();
    result.unmount();
    result.transport.dispose();
  }
);
