import type { HostRecord } from "@core/generated/bindings";
import type { ScenarioName } from "@core/mock/fixtures";
import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { HostSelectionContext } from "~/lib/host-selection";
import { HostStoreProvider } from "~/stores/host-store";
import { PowerDrawCard } from "./power-draw-card";

const NOW = 1_800_000_000_000;
const APPS_NOTE =
  "App lines are CPU energy as macOS estimates it, for your own processes, so they add up to less than system draw.";

async function renderCard(scenarios: ScenarioName[] = [], now = () => NOW) {
  const r = renderWithProviders(<PowerDrawCard windowMs={900_000} />, {
    transportOptions: { now, scenarios },
  });
  await screen.findByText("Power draw, last 15 minutes");
  act(() => r.transport.tick());
  return r;
}

const moves = (d: string | null | undefined) =>
  (d ?? "").match(/M/g)?.length ?? 0;
const line = (container: HTMLElement, key: string) =>
  container.querySelector(`[data-series="${key}"]`);

describe("PowerDrawCard CPU power note (D-054, D-065)", () => {
  it("says the CPU figure is P cores, uncalibrated, for source 1", async () => {
    await renderCard(["cpu-power-uncalibrated"]);
    expect(
      screen.getByText("CPU power: P cores, uncalibrated")
    ).toBeInTheDocument();
  });

  it("says the CPU figure is a carried-over estimate for source 3", async () => {
    await renderCard(["cpu-power-seeded"]);
    expect(
      screen.getByText("CPU power: P cores, estimated from last calibration")
    ).toBeInTheDocument();
  });

  it("still says P cores once calibrated (source 2)", async () => {
    await renderCard(["cpu-power-calibrated"]);
    expect(screen.getByText("CPU power: P cores")).toBeInTheDocument();
  });

  it("says nothing when CPU power is measured directly", async () => {
    await renderCard([]);
    expect(screen.queryByText(/^CPU power/)).toBeNull();
  });
});

describe("PowerDrawCard system line", () => {
  // Guards against putting the sparse PMP components (ANE, DRAM, package;
  // D-043) back on the chart: the line drawn is `power.system` alone. The
  // stack's broken one-point runs are covered by StreamArea's own test.
  it("stays one continuous line while ANE and DRAM are mostly gaps", async () => {
    let clock = NOW;
    const { container, transport } = await renderCard([], () => clock);
    act(() => {
      for (let t = 0; t < 40; t++) {
        clock += 1000;
        // Measured on one tick in ten, as when the PMP counters move.
        const gapped = t % 10 !== 0;
        transport.setReadFailing("power.ane", gapped);
        transport.setReadFailing("power.dram", gapped);
        transport.setReadFailing("power.package", gapped);
        transport.tick();
      }
    });
    const system = line(container, "power.system");
    const d = system?.querySelector("[data-line]")?.getAttribute("d");
    expect(moves(d)).toBe(1);
    expect(container.querySelectorAll("[data-gap-edge]")).toHaveLength(0);
    expect(line(container, "power.ane")).toBeNull();
    expect(line(container, "power.dram")).toBeNull();
    // The components are honest gaps in the strip.
    const ane = screen.getByText("ANE").nextElementSibling;
    expect(ane).toHaveTextContent("—");
  });
});

describe("PowerDrawCard app lines (D-093)", () => {
  it("draws the top apps as bare lines and focuses one from the legend", async () => {
    const { container, user } = await renderCard();
    await screen.findByText(APPS_NOTE);
    const apps = container.querySelectorAll('[data-series^="app:"]');
    expect(apps).toHaveLength(5);
    for (const g of apps) {
      expect(g.querySelector("[data-area]")).toBeNull();
      expect(g.querySelector("[data-line]")?.getAttribute("stroke-width")).toBe(
        "1"
      );
    }
    // Steps repeat after three apps; the repeats are dashed.
    const dashes = [...apps].map((g) =>
      g.querySelector("[data-line]")?.getAttribute("stroke-dasharray")
    );
    expect(dashes).toEqual([null, null, null, "3 2", "3 2"]);
    const first = apps[0]?.getAttribute("data-series")?.slice(4) ?? "";
    await user.click(screen.getByRole("button", { name: new RegExp(first) }));
    await waitFor(() =>
      expect(
        line(container, `app:${first}`)
          ?.querySelector("[data-line]")
          ?.getAttribute("stroke-width")
      ).toBe("2")
    );
    const others = [
      ...container.querySelectorAll('[data-series^="app:"]'),
    ].filter((g) => g.getAttribute("data-series") !== `app:${first}`);
    for (const g of others) expect(g.getAttribute("opacity")).toBe("0.3");
    // The system line keeps its emphasis.
    expect(line(container, "power.system")?.getAttribute("opacity")).toBe("1");
  });

  it("says so, and draws the system line only, when the apps fail to load", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    transport.queryUsageSeries = async () => ({
      status: "error",
      error: { kind: "invalid_argument", message: "boom" },
    });
    const { container } = renderWithProviders(
      <PowerDrawCard windowMs={900_000} />,
      { transport }
    );
    act(() => transport.tick());
    expect(
      await screen.findByText("Couldn't load per-app power.")
    ).toBeInTheDocument();
    expect(container.querySelector('[data-series^="app:"]')).toBeNull();
    expect(line(container, "power.system")).not.toBeNull();
  });

  it("says per-app power comes later when no app was sampled yet", async () => {
    const transport = createMockTransport({ now: () => NOW, autoTick: false });
    transport.queryUsageSeries = async (_host, fromMs) => ({
      status: "ok",
      data: {
        from_ms: fromMs,
        bucket_ms: 10_000,
        since_ms: null,
        complete_to_ms: null,
        latest_ms: null,
        apps: [],
      },
    });
    const { container } = renderWithProviders(
      <PowerDrawCard windowMs={900_000} />,
      { transport }
    );
    act(() => transport.tick());
    expect(
      await screen.findByText(
        "Per-app power appears after Kelvo has sampled processes for a while."
      )
    ).toBeInTheDocument();
    expect(container.querySelector('[data-series^="app:"]')).toBeNull();
  });

  it("shows the system line only, with a note, on a remote host", async () => {
    const transport = createMockTransport({
      now: () => NOW,
      scenarios: ["multi-host"],
      autoTick: false,
    });
    const hosts = await transport.listHosts();
    const remote = hosts.find((h) => !h.is_local) as HostRecord;
    const { container } = renderWithProviders(
      <HostSelectionContext
        value={{ hosts, selected: remote.id, select: () => {} }}
      >
        <HostStoreProvider hostId={remote.id}>
          <PowerDrawCard windowMs={900_000} />
        </HostStoreProvider>
      </HostSelectionContext>,
      { transport }
    );
    expect(
      await screen.findByText("Per-app power isn't available for remote hosts.")
    ).toBeInTheDocument();
    expect(container.querySelector('[data-series^="app:"]')).toBeNull();
    expect(screen.queryByText(APPS_NOTE)).toBeNull();
  });
});
