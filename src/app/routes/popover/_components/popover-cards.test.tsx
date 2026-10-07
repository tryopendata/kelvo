import type { LiveMsg } from "@core/generated/bindings";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { type HostStore, useHostStore } from "~/stores/host-store";
import { LiveCpuCard, LiveMemoryCard, LivePowerCard } from "./popover-cards";

let storeRef: HostStore | null = null;
function GrabStore() {
  storeRef = useHostStore();
  return null;
}

/** A frame like the last one, with `missing` series not measured. */
function frameWithout(missing: string[]): LiveMsg {
  const state = storeRef?.getState();
  const layoutNo = state?.layoutNo;
  if (!state || layoutNo == null) throw new Error("no layout");
  const keys = state.layouts[layoutNo]?.keys ?? [];
  const values = keys.map((k) =>
    missing.includes(k) ? null : (state.held[k] ?? null)
  );
  return {
    kind: "frame",
    ts_ms: (state.lastTsMs ?? 0) + 1000,
    layout_no: layoutNo,
    timeline: 0,
    values,
    held: values,
  };
}

describe("popover memory card", () => {
  it("shows used memory in the GB/GiB setting next to the marketing total", async () => {
    const { transport } = renderWithProviders(
      <>
        <GrabStore />
        <LiveMemoryCard />
      </>
    );
    await screen.findByText(/^\/ \d+ GB$/);
    const used = () => storeRef?.getState().held["mem.used"] ?? null;
    // The headline figure is the text before the " / 24 GB" unit.
    const value = () =>
      screen.getByText(/^\/ \d+ GB$/).parentElement?.firstChild?.textContent;
    await waitFor(() => expect(used()).not.toBeNull());
    expect(value()).toBe(((used() as number) / 1e9).toFixed(1));

    await act(() => transport.updateSettings({ units: { memory: "binary" } }));
    await waitFor(() =>
      expect(value()).toBe(((used() as number) / 2 ** 30).toFixed(1))
    );
    expect(screen.getByText(/^\/ \d+ GB$/)).toBeInTheDocument();
  });
});

describe("popover cards with missing values", () => {
  it("draws a missing CPU user share as an empty track, not a 0 bar", async () => {
    const { transport, container } = renderWithProviders(
      <>
        <GrabStore />
        <LiveCpuCard />
      </>
    );
    await screen.findByText("User");
    expect(container.querySelector("[data-missing]")).toBeNull();

    act(() => transport.push(frameWithout(["cpu.user"])));
    const row = screen.getByText("User").parentElement as HTMLElement;
    expect(row.querySelector("[data-missing]")).toHaveClass("bg-track");
    expect(row.querySelector("[style*=scaleX]")).toBeNull();
    // System is still measured and keeps its fill.
    const system = screen.getByText("System").parentElement as HTMLElement;
    expect(system.querySelector("[style*=scaleX]")).not.toBeNull();
  });

  it("does not fold a missing rail into the rest of the system", async () => {
    const { transport } = renderWithProviders(
      <>
        <GrabStore />
        <LivePowerCard />
      </>
    );
    const bar = await screen.findByRole("img", { name: /Rest of system/ });
    expect(bar.getAttribute("aria-label")).toMatch(/Rest of system [\d.]+ W/);
    const slices = bar.children.length;

    act(() => transport.push(frameWithout(["power.gpu"])));
    const after = screen.getByRole("img", { name: /Rest of system/ });
    // GPU and the rest are unknown and drop out; the others keep their slices.
    expect(after.getAttribute("aria-label")).toContain("GPU —");
    expect(after.getAttribute("aria-label")).toContain("Rest of system —");
    expect(after.children).toHaveLength(slices - 2);
  });
});
