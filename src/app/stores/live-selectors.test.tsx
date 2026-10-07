import type { LiveMsg } from "@core/generated/bindings";
import { act, screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useHostStore } from "./host-store";
import { useCpu, useMemory } from "./live-selectors";

const renders = { cpu: 0, memory: 0 };

function CpuCard() {
  renders.cpu++;
  const cpu = useCpu();
  return <p>cpu {cpu.total ?? "none"}</p>;
}

function MemoryCard() {
  renders.memory++;
  const mem = useMemory();
  return <p>mem {mem.pressure ?? "none"}</p>;
}

let storeRef: ReturnType<typeof useHostStore> | null = null;
function GrabStore() {
  storeRef = useHostStore();
  return null;
}

/**
 * The 1 Hz selector contract (frontend/testing.md): a frame that moves only
 * CPU values must not re-render a card that reads only Memory.
 */
describe("per-module selectors", () => {
  it("re-renders only the card whose slice changed", async () => {
    const { transport } = renderWithProviders(
      <>
        <GrabStore />
        <CpuCard />
        <MemoryCard />
      </>
    );
    await screen.findByText(/^cpu \d/);
    const state = storeRef?.getState();
    const layoutNo = state?.layoutNo;
    if (!state || layoutNo == null) throw new Error("no layout");
    const keys = state.layouts[layoutNo]?.keys ?? [];
    const held = keys.map((k) => state.held[k] ?? null);
    const cpuIdx = keys.indexOf("cpu.total");
    expect(cpuIdx).toBeGreaterThanOrEqual(0);

    const frame = (ts: number, cpu: number): LiveMsg => {
      const values = [...held];
      values[cpuIdx] = cpu;
      return {
        kind: "frame",
        ts_ms: ts,
        layout_no: layoutNo,
        timeline: 0,
        values,
        held: values,
      };
    };

    const base = (state.lastTsMs ?? 0) + 1000;
    renders.cpu = 0;
    renders.memory = 0;
    act(() => transport.push(frame(base, 91)));
    act(() => transport.push(frame(base + 1000, 92)));

    expect(screen.getByText("cpu 92")).toBeInTheDocument();
    expect(renders.cpu).toBe(2);
    expect(renders.memory).toBe(0);
  });
});
