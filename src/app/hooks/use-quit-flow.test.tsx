import { createMockTransport } from "@core/mock-transport";
import type { ProcessTarget } from "@core/process-selection";
import { act, screen, waitFor } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { useEdition } from "./use-edition";
import { useQuitFlow } from "./use-quit-flow";

const targets: ProcessTarget[] = [
  { pid: 40, start_time_us: 100, name: "python", refusal: null },
  { pid: 41, start_time_us: 200, name: "python", refusal: null },
];
function Harness({ items = targets }: { items?: ProcessTarget[] }) {
  const flow = useQuitFlow();
  const edition = useEdition();
  const [source, setSource] = useState(items);
  return (
    <>
      <button
        type="button"
        disabled={!edition?.process_signal}
        onClick={() => flow.requestMany(source, "quit")}
      >
        Request
      </button>
      <button
        type="button"
        onClick={() => {
          void flow.confirm();
          void flow.confirm();
        }}
      >
        Confirm twice
      </button>
      <button type="button" onClick={flow.cancel}>
        Cancel
      </button>
      <button
        type="button"
        onClick={() =>
          setSource([
            { pid: 40, start_time_us: 999, name: "replacement", refusal: null },
          ])
        }
      >
        Replace source
      </button>
      <output>
        {flow.pending
          ? flow.pending.targets
              .map((p) => `${p.pid}:${p.start_time_us}`)
              .join(",")
          : "No pending request"}
      </output>
      <output>
        {flow.results.map((r) => `${r.target.pid}:${r.state}`).join(",")}
      </output>
    </>
  );
}

describe("quit batch ownership", () => {
  it("locks synchronously and sends the captured identity even after the source changes", async () => {
    const transport = createMockTransport({ autoTick: false });
    const sent: [number, number][] = [];
    let release: (() => void) | undefined;
    transport.processSignal = async (_host, pid, start) => {
      sent.push([pid, start]);
      if (pid === 40)
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      return { status: "ok", data: null };
    };
    const { user } = renderWithProviders(<Harness />, { transport });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Request" })).toBeEnabled()
    );
    await user.click(screen.getByRole("button", { name: "Request" }));
    await user.click(screen.getByRole("button", { name: "Replace source" }));
    expect(screen.getByText("40:100,41:200")).toBeVisible();
    await user.click(screen.getByRole("button", { name: "Confirm twice" }));
    expect(sent).toEqual([[40, 100]]);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByText("40:100,41:200")).toBeVisible();
    await act(async () => release?.());
    expect(sent).toEqual([
      [40, 100],
      [41, 200],
    ]);
    expect(screen.getByText("40:accepted,41:accepted")).toBeVisible();
  });

  it("skips known exclusions and continues after PID reuse without touching its replacement", async () => {
    const transport = createMockTransport({ autoTick: false });
    const sent: number[] = [];
    transport.processSignal = async (_host, pid) => {
      sent.push(pid);
      return { status: "error", error: { kind: "pid_reused" } };
    };
    const { user } = renderWithProviders(
      <Harness
        items={[
          ...targets,
          {
            pid: 0,
            start_time_us: 0,
            name: "kernel_task",
            refusal: "kernel_task",
          },
          {
            pid: 50,
            start_time_us: 0,
            name: "exited",
            refusal: null,
            running: false,
          },
        ]}
      />,
      { transport }
    );
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Request" })).toBeEnabled()
    );
    await user.click(screen.getByRole("button", { name: "Request" }));
    await user.click(screen.getByRole("button", { name: "Confirm twice" }));
    expect(sent).toEqual([40, 41]);
    expect(
      screen.getByText("40:failed,41:failed,0:excluded,50:excluded")
    ).toBeVisible();
  });

  it("stops the rest of the batch when the edition becomes unavailable", async () => {
    const transport = createMockTransport({ autoTick: false });
    const sent: number[] = [];
    transport.processSignal = async (_host, pid) => {
      sent.push(pid);
      return { status: "error", error: { kind: "unavailable" } };
    };
    const { user } = renderWithProviders(<Harness />, { transport });
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Request" })).toBeEnabled()
    );
    await user.click(screen.getByRole("button", { name: "Request" }));
    await user.click(screen.getByRole("button", { name: "Confirm twice" }));
    expect(sent).toEqual([40]);
    expect(screen.getByText("40:failed,41:unsent")).toBeVisible();
    expect(screen.getByRole("button", { name: "Request" })).toBeDisabled();
  });
});
