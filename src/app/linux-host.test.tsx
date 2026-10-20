import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import type { ReactElement } from "react";
import CpuRoute from "~/routes/dashboard/cpu/route";
import DiskRoute from "~/routes/dashboard/disk/route";
import MemoryRoute from "~/routes/dashboard/memory/route";
import OverviewRoute from "~/routes/dashboard/overview/route";
import PowerRoute from "~/routes/dashboard/power/route";
import ProcessesRoute from "~/routes/dashboard/processes/route";
import { HostStoreProvider } from "~/stores/host-store";

const NOW = 1_800_000_000_000;

/** A route on a Linux host (the `linux` mock, D-115) after one live frame. */
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

describe("a Linux host (D-115)", () => {
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
    expect(
      await screen.findByRole("heading", { name: "Pressure stall" })
    ).toBeVisible();
    expect(screen.queryByText("Compressed")).toBeNull();
    expect(screen.queryByText("Wired")).toBeNull();
    expect(screen.queryByText("GB unified")).toBeNull();
    expect(screen.getByText("Cached files")).toBeVisible();
  });

  it("lists temperature sensors and system power in place of SoC zones and the component stack", async () => {
    await linux(<PowerRoute />, "Power & Sensors");
    expect(
      await screen.findByRole("heading", { name: "Temperature sensors" })
    ).toBeVisible();
    expect(screen.getByText("NAND Flash Temperature")).toBeVisible();
    expect(
      screen.getByRole("heading", { name: /^System power, last/ })
    ).toBeVisible();
    expect(screen.getByText("AC Input Power")).toBeVisible();
    expect(screen.queryByRole("heading", { name: "CPU" })).toBeNull();
    expect(screen.queryByRole("heading", { name: "GPU" })).toBeNull();
    expect(screen.queryByText(/^Power by component/)).toBeNull();
    expect(screen.queryByText(/as macOS estimates it/)).toBeNull();
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
