import type { ChartWindow } from "@core/generated/bindings";
import { MOCK_HOST_ID } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import type { ReactElement } from "react";
import CpuRoute from "~/routes/dashboard/cpu/route";
import DiskRoute from "~/routes/dashboard/disk/route";
import GpuRoute from "~/routes/dashboard/gpu/route";
import MemoryRoute from "~/routes/dashboard/memory/route";
import NetworkRoute from "~/routes/dashboard/network/route";
import PowerRoute from "~/routes/dashboard/power/route";
import { HostStoreProvider } from "~/stores/host-store";

const NOW = 1_800_000_000_000;
const FROM = NOW - 86_400_000;
const TO = NOW - 60_000;
const FIXED: ChartWindow = { absolute: { from_ms: FROM, to_ms: TO } };
const PAGES = [
  ["CPU", CpuRoute, "cpu"],
  ["GPU", GpuRoute, "gpu"],
  ["Memory", MemoryRoute, "memory"],
  ["Power", PowerRoute, "energy"],
  ["Network", NetworkRoute, "network"],
] as const;
function mount(element: ReactElement, chartWindow: ChartWindow = FIXED) {
  return renderWithProviders(element, {
    transportOptions: { now: () => NOW, chartWindow },
  });
}

it.each(PAGES)(
  "%s fixed ranges show source history without recent app queries or Quit",
  async (_name, Page, by) => {
    const { transport } = mount(<Page />);
    expect(await screen.findByText("Compiler")).toBeVisible();
    expect(transport.calls).toContainEqual({
      command: "query_historical_usage",
      args: [
        { host: MOCK_HOST_ID, from_ms: FROM, to_ms: TO, by, app_names: [] },
      ],
    });
    expect(
      transport.calls.some(
        (call) =>
          call.command === "query_usage_by_app" ||
          call.command === "query_network_by_app"
      )
    ).toBe(false);
    const table = screen.getByRole("table", {
      name: "Recorded apps in this range",
    });
    expect(within(table).queryByRole("checkbox")).toBeNull();
    expect(screen.queryByRole("button", { name: /^Quit/ })).toBeNull();
  }
);

it("a rolling day uses durable attribution beyond the recent live ring", async () => {
  const { transport } = mount(<CpuRoute />, { relative: 86_400_000 });
  expect(await screen.findByText("Compiler")).toBeVisible();
  expect(transport.calls).toContainEqual({
    command: "query_historical_usage",
    args: [
      {
        host: MOCK_HOST_ID,
        from_ms: FROM,
        to_ms: NOW,
        by: "cpu",
        app_names: [],
      },
    ],
  });
  expect(
    transport.calls.some((call) => call.command === "query_usage_by_app")
  ).toBe(false);
});

it("live short ranges keep the existing app search and recent query", async () => {
  const { transport } = mount(<CpuRoute />, "5m");
  expect(
    await screen.findByRole("searchbox", {
      name: "Search CPU by app, process or PID",
    })
  ).toBeVisible();
  await waitFor(() =>
    expect(
      transport.calls.some((call) => call.command === "query_usage_by_app")
    ).toBe(true)
  );
  expect(
    transport.calls.some((call) => call.command === "query_historical_usage")
  ).toBe(false);
});

it("Disk keeps its historical charts and explicitly omits unrecorded app attribution", async () => {
  const { transport } = mount(<DiskRoute />);
  expect(
    await screen.findByText("Per-app disk history isn’t recorded")
  ).toBeVisible();
  expect(screen.getByRole("heading", { name: "Disk I/O" })).toBeVisible();
  await waitFor(() =>
    expect(transport.calls).toContainEqual({
      command: "query_disk_io",
      args: [
        expect.objectContaining({
          host: MOCK_HOST_ID,
          from_ms: FROM,
          to_ms: TO,
        }),
      ],
    })
  );
  expect(
    transport.calls.some(
      (call) =>
        call.command === "query_usage_by_app" ||
        call.command === "query_historical_usage"
    )
  ).toBe(false);
  expect(screen.queryByRole("table", { name: /Disk processes/ })).toBeNull();
});

it("Network reads the capable remote source for a fixed interval", async () => {
  const transport = createMockTransport({
    now: () => NOW,
    chartWindow: FIXED,
    scenarios: ["multi-host"],
  });
  renderWithProviders(
    <HostStoreProvider hostId="remote-studio">
      <NetworkRoute />
    </HostStoreProvider>,
    { transport }
  );
  expect(await screen.findByText("Compiler")).toBeVisible();
  expect(transport.calls).toContainEqual({
    command: "query_historical_usage",
    args: [
      {
        host: "remote-studio",
        from_ms: FROM,
        to_ms: TO,
        by: "network",
        app_names: [],
      },
    ],
  });
  expect(screen.queryByRole("heading", { name: "Apps, now" })).toBeNull();
});

it("turning Network recording off does not substitute current rates for a fixed interval", async () => {
  const transport = createMockTransport({ now: () => NOW, chartWindow: FIXED });
  await transport.updateSettings({ history: { network_history: false } });
  renderWithProviders(<NetworkRoute />, { transport });
  expect(await screen.findByText("Compiler")).toBeVisible();
  expect(
    transport.calls.some((call) => call.command === "query_historical_usage")
  ).toBe(true);
  expect(
    transport.calls.some((call) => call.command === "query_network_by_app")
  ).toBe(false);
});

it("returning to Live restores recent app controls instead of holding the fixed table", async () => {
  const { transport } = mount(<CpuRoute />);
  await screen.findByText("Compiler");
  await act(() =>
    transport.updateSettings({ general: { chart_window: "5m" } })
  );
  expect(
    await screen.findByRole("searchbox", {
      name: "Search CPU by app, process or PID",
    })
  ).toBeVisible();
  await waitFor(() =>
    expect(
      transport.calls.some((call) => call.command === "query_usage_by_app")
    ).toBe(true)
  );
});

it("a historical remote failure stays explicit and Retry restores the same interval", async () => {
  const transport = createMockTransport({
    now: () => NOW,
    chartWindow: FIXED,
    scenarios: ["multi-host"],
  });
  const read = transport.queryHistoricalUsage;
  let failing = true;
  transport.queryHistoricalUsage = async (request) =>
    failing
      ? {
          status: "error",
          error: {
            kind: "remote_unsupported",
            message: "Older peer has no durable attribution",
          },
        }
      : read(request);
  const { user } = renderWithProviders(
    <HostStoreProvider hostId="remote-studio">
      <NetworkRoute />
    </HostStoreProvider>,
    { transport }
  );
  expect(await screen.findByRole("alert")).toHaveTextContent(
    "doesn't offer recorded app usage"
  );
  expect(
    screen.queryByRole("table", { name: "Network by app, now" })
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "Reset history" })).toBeNull();
  failing = false;
  await user.click(screen.getByRole("button", { name: "Retry" }));
  expect(await screen.findByText("Compiler")).toBeVisible();
  expect(transport.calls).toContainEqual({
    command: "query_historical_usage",
    args: [
      {
        host: "remote-studio",
        from_ms: FROM,
        to_ms: TO,
        by: "network",
        app_names: [],
      },
    ],
  });
});
