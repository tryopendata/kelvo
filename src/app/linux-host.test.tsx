import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import type { ReactElement } from "react";
import { LiveSidebar } from "~/routes/dashboard/_components/live-sidebar";
import ContainersRoute from "~/routes/dashboard/containers/route";
import CpuRoute from "~/routes/dashboard/cpu/route";
import DiskRoute from "~/routes/dashboard/disk/route";
import MemoryRoute from "~/routes/dashboard/memory/route";
import OverviewRoute from "~/routes/dashboard/overview/route";
import PowerRoute from "~/routes/dashboard/power/route";
import ProcessesRoute from "~/routes/dashboard/processes/route";
import { HostStoreProvider } from "~/stores/host-store";

const NOW = 1_800_000_000_000;

/** A route on a Linux host (the `linux` mock, D-116) after one live frame. */
async function linux(ui: ReactElement, heading: string, route = "/") {
  const r = renderWithProviders(ui, {
    route,
    transportOptions: { now: () => NOW, scenarios: ["linux"] },
  });
  await screen.findByRole("heading", { name: heading });
  act(() => r.transport.tick());
  return r;
}

async function mac(ui: ReactElement, heading: string) {
  const r = renderWithProviders(ui, { transportOptions: { now: () => NOW } });
  await screen.findByRole("heading", { name: heading });
  act(() => r.transport.tick());
  return r;
}

describe("a Linux host (D-116)", () => {
  it("heads the Overview with its distribution and kernel, not macOS", async () => {
    await linux(<OverviewRoute />, "Overview");
    const header = await screen.findByRole("region", { name: "This host" });
    expect(header).toHaveTextContent(
      "Fedora Linux Asahi Remix 41 (Server Edition)"
    );
    expect(header).toHaveTextContent("6.17.12-400.asahi.fc41.aarch64+16k");
    expect(header).not.toHaveTextContent("macOS");
    expect(header).not.toHaveTextContent("unified");
    expect(screen.getByText("Not available on this host")).toBeVisible();
  });

  it("builds the Overview memory and power cards from what the host reports", async () => {
    await linux(<OverviewRoute />, "Overview");
    expect(await screen.findByText("Pressure stall")).toBeVisible();
    expect(screen.getByText("Hottest sensor")).toBeVisible();
    const sensors = screen.getByRole("list", {
      name: "Temperature sensors, hottest first",
    });
    expect(within(sensors).getByText("WiFi/BT Module Temp")).toBeVisible();
  });

  it("adds I/O wait, steal and pressure stall to the CPU strip", async () => {
    await linux(<CpuRoute />, "CPU");
    for (const label of ["I/O wait", "Steal", "Pressure stall", "Load avg"]) {
      expect(await screen.findByText(label)).toBeVisible();
    }
  });

  it("leaves the Mac CPU strip as it was", async () => {
    await mac(<CpuRoute />, "CPU");
    expect(await screen.findByText("Idle")).toBeVisible();
    expect(screen.queryByText("I/O wait")).toBeNull();
    expect(screen.queryByText("Pressure stall")).toBeNull();
  });

  it("shows used, cached and free memory with PSI, without the macOS split", async () => {
    await linux(<MemoryRoute />, "Memory");
    const strip = await screen.findByRole("region", {
      name: "Memory composition",
    });
    expect(within(strip).getByText("Pressure stall")).toBeVisible();
    expect(screen.queryByText("Compressed")).toBeNull();
    expect(screen.queryByText("Wired")).toBeNull();
    expect(screen.queryByText("GB unified")).toBeNull();
    expect(within(strip).getByText("Cached files")).toBeVisible();
  });

  it("charts used with cached files stacked on it up to MemTotal, in place of pressure", async () => {
    await linux(<MemoryRoute />, "Memory");
    const card = await screen.findByRole("region", { name: "Used" });
    const chart = within(card).getByRole("img", {
      name: /^Memory used and cached files, last 15 minutes, of 16\.5 GB, used [\d.]+ GB, cached [\d.]+ GB\./,
    });
    expect(chart.querySelector('[data-series="mem.used"]')).not.toBeNull();
    expect(chart.querySelector('[data-series="mem.cached"]')).not.toBeNull();
    expect(within(card).getByText("Cached files")).toBeVisible();
    expect(within(card).getByText("16.5 GB")).toBeInTheDocument();
    expect(
      within(card).getByRole("slider", { name: "Select a time range" })
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: /^Pressure/ })).toBeNull();
  });

  it("keeps the Mac Memory page's pressure chart", async () => {
    await mac(<MemoryRoute />, "Memory");
    expect(
      await screen.findByRole("img", { name: /^Memory pressure, last/ })
    ).toBeVisible();
    expect(screen.getByRole("heading", { name: "Pressure" })).toBeVisible();
    expect(screen.queryByRole("region", { name: "Used" })).toBeNull();
  });

  it("lists temperature sensors and power rails in place of SoC zones and the components", async () => {
    await linux(<PowerRoute />, "Power & Sensors");
    expect(
      await screen.findByRole("heading", { name: /^Temperature sensors, last/ })
    ).toBeVisible();
    expect(screen.getByText("NAND Flash Temperature")).toBeVisible();
    expect(
      screen.getByRole("heading", { name: /^Power draw, last/ })
    ).toBeInTheDocument();
    expect(screen.getByText("AC Input Power")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "CPU" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "GPU" })).toBeNull();
    expect(screen.queryByText("ANE")).toBeNull();
    expect(screen.queryByText(/as macOS estimates it/)).toBeNull();
    expect(screen.queryByText(/^Energy by app/)).toBeNull();
    // No ANE, so no ANE spike marker on the chart.
    expect(screen.queryByText(/Photos face analysis/)).toBeNull();
  });

  it("offers only the process columns the host measures", async () => {
    const r = await linux(<ProcessesRoute />, "Processes");
    const sets = screen.getByRole("radiogroup", { name: "Columns" });
    expect(within(sets).queryByRole("radio", { name: "Energy" })).toBeNull();
    expect(within(sets).queryByRole("radio", { name: "Network" })).toBeNull();
    expect(within(sets).queryByRole("radio", { name: "GPU" })).toBeNull();
    await r.user.click(within(sets).getByRole("radio", { name: "Memory" }));
    expect(
      screen.queryByRole("columnheader", { name: /Compressed/ })
    ).toBeNull();
    expect(screen.queryByText(/macOS only lets administrators/)).toBeNull();
  });

  it("lists a remote Linux host's block devices from its capabilities", async () => {
    const transport = createMockTransport({
      now: () => NOW,
      autoTick: false,
      chartWindow: "5m",
      scenarios: ["linux-host"],
    });
    const { user } = renderWithProviders(
      <HostStoreProvider hostId="remote-linux">
        <DiskRoute />
      </HostStoreProvider>,
      { transport, route: "/dashboard/disk" }
    );
    const device = await screen.findByRole("combobox", {
      name: "Disk device",
    });
    await waitFor(() =>
      expect(device).toHaveTextContent("APPLE SSD AP0512Q · nvme0n1")
    );
    await user.click(device);
    expect(
      await screen.findByRole("option", { name: "Samsung PSSD T7 · sda" })
    ).toBeVisible();
    expect(
      screen.getByRole("option", { name: "WDC WD40EFRX-68N · sdb" })
    ).toBeVisible();
  });
});

describe("Use by app on a remote Linux host (D-117)", () => {
  it("lists its apps from the host's last hour, with Linux footnotes and no Quit", async () => {
    const transport = createMockTransport({
      now: () => NOW,
      autoTick: false,
      chartWindow: "5m",
      scenarios: ["linux-host"],
    });
    renderWithProviders(
      <HostStoreProvider hostId="remote-linux">
        <MemoryRoute />
      </HostStoreProvider>,
      { transport, route: "/dashboard/memory" }
    );
    const table = await screen.findByRole("table", { name: "Memory by app" });
    // Every user's processes: postgres runs as its own user.
    await waitFor(() =>
      expect(within(table).getByText("postgres")).toBeVisible()
    );
    expect(table).toHaveTextContent("Resident memory (RSS), as top and ps");
    expect(table).not.toHaveTextContent("Activity Monitor");
    expect(table).not.toHaveTextContent("macOS");
    expect(within(table).queryByRole("button", { name: /^Quit / })).toBeNull();
    expect(within(table).queryByRole("checkbox")).toBeNull();
    expect(
      transport.calls.some(
        (c) =>
          c.command === "query_usage_by_app" && c.args[0] === "remote-linux"
      )
    ).toBe(true);
  });

  it("names kernel work, not macOS, in the CPU remainder", async () => {
    const transport = createMockTransport({
      now: () => NOW,
      autoTick: false,
      chartWindow: "5m",
      scenarios: ["linux-host"],
    });
    renderWithProviders(
      <HostStoreProvider hostId="remote-linux">
        <CpuRoute />
      </HostStoreProvider>,
      { transport, route: "/dashboard/cpu" }
    );
    const table = await screen.findByRole("table", { name: "CPU by app" });
    expect(table).toHaveTextContent(
      "System and other is the rest of the CPU total: kernel work, processes the agent can't read"
    );
    expect(table).not.toHaveTextContent("macOS");
  });
});

/** The Containers route on the `linux-host` peer after its backfill. */
async function remoteContainers() {
  const transport = createMockTransport({
    now: () => NOW,
    autoTick: false,
    chartWindow: "5m",
    scenarios: ["linux-host"],
  });
  const r = renderWithProviders(
    <HostStoreProvider hostId="remote-linux">
      <ContainersRoute />
    </HostStoreProvider>,
    { transport, route: "/dashboard/containers" }
  );
  const table = await screen.findByRole("table", {
    name: "Services and containers",
  });
  await waitFor(() =>
    expect(within(table).getByText("opendata-worker")).toBeVisible()
  );
  return { ...r, table };
}

function rowOf(table: HTMLElement, name: string): HTMLElement {
  const row = within(table).getByText(name).closest("tr");
  if (!row) throw new Error(`no row for ${name}`);
  return row;
}

describe("Containers on a Linux host (D-116)", () => {
  it("lists cgroup groups by CPU with the memory limit when there is one", async () => {
    const { table } = await remoteContainers();
    const rows = within(table).getAllByRole("row").slice(1);
    expect(rows[0]).toHaveTextContent("opendata-worker");
    expect(rows[0]).toHaveTextContent("Docker");
    expect(rows[0]).toHaveTextContent("2.6");
    expect(rows[0]).toHaveTextContent("/ 4.3 GB");
    expect(rowOf(table, "docker.service")).toHaveTextContent("Service");
    // cadvisor has no `memory.max`: no limit, not "/ 0".
    expect(rowOf(table, "cadvisor")).not.toHaveTextContent(/B \/ /);
    for (const head of [
      "Name",
      "Kind",
      "% CPU",
      "Memory",
      "Disk read",
      "Disk write",
      "Memory pressure",
    ])
      expect(
        within(table).getByRole("columnheader", {
          name: new RegExp(`^${head}( [↑↓])?$`),
        })
      ).toBeVisible();
  });

  it("pins the folded remainder last as its count of groups", async () => {
    const { table, user } = await remoteContainers();
    const last = () => within(table).getAllByRole("row").at(-1);
    expect(last()).toHaveTextContent("25 others");
    expect(within(table).queryByText("_other")).toBeNull();
    await user.click(within(table).getByRole("button", { name: "Memory" }));
    expect(last()).toHaveTextContent("25 others");
  });

  it("shows an exited container as a gap, sorted last, never 0", async () => {
    const { table } = await remoteContainers();
    const row = rowOf(table, "opendata-migrate");
    expect(row).toHaveTextContent("—");
    expect(row).not.toHaveTextContent(/\b0(\.0)?\b/);
    const named = within(table).getAllByRole("row").slice(1, -1);
    expect(named.at(-1)).toBe(row);
  });

  it("separates services from containers", async () => {
    const { table, user } = await remoteContainers();
    const show = screen.getByRole("radiogroup", { name: "Show" });
    await user.click(within(show).getByRole("radio", { name: "Services" }));
    expect(within(table).getByText("docker.service")).toBeVisible();
    expect(within(table).queryByText("opendata-worker")).toBeNull();
    expect(within(table).queryByText(/others$/)).toBeNull();
    await user.click(within(show).getByRole("radio", { name: "Containers" }));
    expect(within(table).getByText("opendata-worker")).toBeVisible();
    expect(within(table).queryByText("sshd.service")).toBeNull();
  });

  it.each([
    ["remote-linux", true],
    ["remote-studio", false],
  ])(
    "lists Containers in the sidebar of %s only when it reports cgroups",
    async (hostId, listed) => {
      const transport = createMockTransport({
        now: () => NOW,
        autoTick: false,
        scenarios: ["linux-host", "multi-host"],
      });
      renderWithProviders(
        <HostStoreProvider hostId={hostId}>
          <LiveSidebar />
        </HostStoreProvider>,
        { transport }
      );
      // Before its record and capabilities arrive every host reads as local.
      await waitFor(() =>
        expect(
          screen.queryByRole("link", { name: /^Battery/ }) === null &&
            screen.queryByRole("link", { name: /^Memory \d+%/ }) !== null
        ).toBe(true)
      );
      await waitFor(() =>
        expect(
          screen.queryByRole("link", { name: "Containers" }) !== null
        ).toBe(listed)
      );
    }
  );
});
