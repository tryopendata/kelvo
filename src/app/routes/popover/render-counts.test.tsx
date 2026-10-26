import type { LiveMsg } from "@core/generated/bindings";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { Profiler } from "react";
import { type HostStore, useHostStore } from "~/stores/host-store";
import {
  LiveCpuCard,
  LiveGpuCard,
  LiveMemoryCard,
  LiveNetworkCard,
  LivePowerCard,
} from "./_components/popover-cards";
import PopoverRoute from "./route";

function CaptureStore({ capture }: { capture: (store: HostStore) => void }) {
  capture(useHostStore());
  return null;
}

function cpuFrame(store: HostStore | undefined): LiveMsg {
  if (!store) throw new Error("Missing live store");
  const state = store.getState();
  if (state.layoutNo === null) throw new Error("Missing live layout");
  const layout = state.layouts[state.layoutNo];
  if (!layout) throw new Error("Missing live layout columns");
  const values = layout.keys.map((key) =>
    key === "cpu.total" ? (state.held[key] ?? 0) + 1 : (state.held[key] ?? null)
  );
  return {
    kind: "frame",
    layout_no: state.layoutNo,
    timeline: 0,
    ts_ms: (state.lastTsMs ?? 0) + 1000,
    values,
    held: values,
  };
}

describe.each([false, true])("popover commits, compact=%s", (compact) => {
  it("commits the complete popover once per incoming frame", async () => {
    let store: HostStore | undefined;
    const commits: number[] = [];
    const { transport } = renderWithProviders(
      <>
        <CaptureStore
          capture={(value) => {
            store = value;
          }}
        />
        <Profiler
          id="popover"
          onRender={(_id, _phase, _actual, _base, _start, commit) =>
            commits.push(commit)
          }
        >
          <PopoverRoute />
        </Profiler>
      </>,
      { transportOptions: { scenarios: compact ? ["popover-compact"] : [] } }
    );
    await screen.findByRole("button", {
      name: `${compact ? "Expand" : "Collapse"} CPU`,
    });
    await waitFor(() => expect(store?.getState().layoutNo).not.toBeNull());
    if (!compact)
      await screen.findByRole("button", { name: /Copy public IP address/ });
    // Drain subscription, settings and query startup before counting live work.
    await act(async () => {});
    for (let tick = 0; tick < 3; tick++) {
      commits.length = 0;
      await act(async () => transport.push(cpuFrame(store)));
      expect(commits).toHaveLength(1);
    }
  });

  it("batches all selectors in each card and leaves unchanged memory alone", async () => {
    let store: HostStore | undefined;
    const commits = new Map<string, number[]>();
    const cards = {
      cpu: LiveCpuCard,
      gpu: LiveGpuCard,
      memory: LiveMemoryCard,
      power: LivePowerCard,
      network: LiveNetworkCard,
    };
    const { transport } = renderWithProviders(
      <>
        <CaptureStore
          capture={(value) => {
            store = value;
          }}
        />
        {Object.entries(cards).map(([id, Card]) => (
          <Profiler
            key={id}
            id={id}
            onRender={(id, _phase, _actual, _base, _start, commit) => {
              const times = commits.get(id) ?? [];
              times.push(commit);
              commits.set(id, times);
            }}
          >
            <Card compact={compact} />
          </Profiler>
        ))}
      </>
    );
    await screen.findByRole("link", { name: "CPU" });
    await waitFor(() => expect(store?.getState().layoutNo).not.toBeNull());
    if (!compact)
      await screen.findByRole("button", { name: /Copy public IP address/ });
    await act(async () => {});
    for (let tick = 0; tick < 3; tick++) {
      commits.clear();
      await act(async () => transport.push(cpuFrame(store)));
      expect(
        Object.fromEntries(
          [...commits].map(([id, times]) => [id, times.length])
        )
      ).toEqual({
        cpu: 1,
        gpu: 1,
        network: 1,
        ...(compact ? { power: 1 } : {}),
      });
      expect(new Set([...commits.values()].flat()).size).toBe(1);
    }
  });
});
