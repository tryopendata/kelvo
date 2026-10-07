import type { LiveProcess } from "@core/generated/bindings";
import type { ScenarioName } from "@core/mock/fixtures";
import { PROCESSES, withGpuPct, withNetRates } from "@core/mock/fixtures";
import { act, screen, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Profiler } from "react";
import { OverviewGpuCard, OverviewNetworkCard } from "./overview-cards";

function renderCard(scenarios: ScenarioName[]) {
  const renders = { n: 0 };
  const view = renderWithProviders(
    <Profiler id="net" onRender={() => renders.n++}>
      <OverviewNetworkCard origin="tl" />
    </Profiler>,
    { transportOptions: { scenarios } }
  );
  return { ...view, renders };
}

const batch = (ts: number, rows: LiveProcess[]) => ({
  kind: "processes" as const,
  ts_ms: ts,
  rows,
});

describe("Overview Network card", () => {
  it("lists your processes by network rate when the host has per-process network", async () => {
    const { transport } = renderCard(["default"]);
    await screen.findByRole("heading", { name: "Network" });
    act(() => transport.push(batch(1, withNetRates(PROCESSES))));
    const list = await screen.findByRole("list", {
      name: "Top processes by network rate",
    });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent)).toEqual([
      "SSafari22.1 MB/s",
      "ccom.docker.backend9.6 MB/s",
      "nnode4.2 MB/s",
      "XXcode1.8 MB/s",
      "FFigma700 KB/s",
    ]);
    expect(screen.getByText("Your processes only")).toBeInTheDocument();
  });

  it("keeps the interface list, and ignores process batches, without it", async () => {
    const { transport, renders } = renderCard(["no-process-network"]);
    await screen.findByRole("list", { name: "Interfaces by total rate" });
    expect(screen.queryByText("Your processes only")).toBeNull();
    const before = renders.n;
    act(() => transport.push(batch(1, withNetRates(PROCESSES))));
    act(() => transport.push(batch(2, PROCESSES)));
    expect(renders.n).toBe(before);
  });
});

function renderGpuCard(scenarios: ScenarioName[]) {
  const renders = { n: 0 };
  const view = renderWithProviders(
    <Profiler id="gpu" onRender={() => renders.n++}>
      <OverviewGpuCard origin="tl" />
    </Profiler>,
    { transportOptions: { scenarios, autoTick: false } }
  );
  return { ...view, renders };
}

describe("Overview GPU card (D-085)", () => {
  it("lists your processes by GPU share when the host has per-process GPU", async () => {
    const { transport } = renderGpuCard(["default"]);
    await screen.findByRole("heading", { name: "GPU" });
    // The first sample after the view opens is a baseline: no shares yet.
    act(() => transport.push(batch(1, PROCESSES)));
    expect(
      await screen.findByText("Measuring GPU by process…")
    ).toBeInTheDocument();
    act(() => transport.push(batch(2, withGpuPct(PROCESSES))));
    const list = await screen.findByRole("list", {
      name: "Top processes by GPU",
    });
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((r) => r.textContent)
    ).toEqual([
      "WWindowServer14.2%",
      "FFigma9.8%",
      "SSafari5.1%",
      "XXcode3.4%",
      "ccom.docker.backend0.6%",
    ]);
    expect(screen.getByText("Your processes only")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /GPU, last/ })).toBeNull();
  });

  it("keeps the 60 s chart, and ignores process batches, without it", async () => {
    const { transport, renders } = renderGpuCard(["no-process-gpu"]);
    await screen.findByRole("heading", { name: "GPU" });
    expect(screen.getByRole("img", { name: /GPU, last/ })).toBeInTheDocument();
    expect(
      screen.queryByRole("list", { name: "Top processes by GPU" })
    ).toBeNull();
    const before = renders.n;
    act(() => transport.push(batch(1, withGpuPct(PROCESSES))));
    act(() => transport.push(batch(2, PROCESSES)));
    expect(renders.n).toBe(before);
  });
});
