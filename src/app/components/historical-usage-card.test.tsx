import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { useState } from "react";
import { HistoricalUsageCard } from "./historical-usage-card";

const NOW = 1_800_000_000_000;
const RANGE = { fromMs: NOW - 600_000, toMs: NOW - 10_000 };
function setup(compare = false) {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  const apps = vi.spyOn(transport, "queryHistoricalUsage");
  const processes = vi.spyOn(transport, "queryUsageProcesses");
  const rendered = renderWithProviders(
    <HistoricalUsageCard range={RANGE} by="cpu" compare={compare} />,
    { transport }
  );
  return { ...rendered, apps, processes };
}

it("ranks by the selected resource and reverses the same app set without another read", async () => {
  const { user, apps } = setup();
  const table = await screen.findByRole("table", {
    name: "Recorded apps in this range",
  });
  expect(within(table).getAllByRole("row")[1]).toHaveTextContent("Compiler");
  await user.click(screen.getByRole("button", { name: "Avg CPU" }));
  expect(within(table).getAllByRole("row")[1]).toHaveTextContent("Editor");
  expect(apps).toHaveBeenCalledTimes(1);
  await user.click(screen.getByRole("button", { name: "Peak RAM" }));
  await waitFor(() =>
    expect(apps).toHaveBeenLastCalledWith(
      expect.objectContaining({ by: "memory" })
    )
  );
  expect(
    screen.queryByRole("button", { name: /Quit|Terminate/ })
  ).not.toBeInTheDocument();
});

it("compares the current membership against exactly the preceding equal interval", async () => {
  const { apps } = setup(true);
  await waitFor(() => expect(apps).toHaveBeenCalledTimes(2));
  expect(apps.mock.calls[1]?.[0]).toEqual(
    expect.objectContaining({
      from_ms: RANGE.fromMs - (RANGE.toMs - RANGE.fromMs),
      to_ms: RANGE.fromMs,
      app_names: ["Compiler", "Browser", "Editor"],
    })
  );
  await screen.findAllByText(/Previous .*pp/);
});

it("loads process instances lazily and refresh discards old continuation pages", async () => {
  const { user, processes } = setup();
  await screen.findByText("Compiler");
  expect(processes).not.toHaveBeenCalled();
  await user.click(
    screen.getByRole("button", { name: "Show Compiler recorded processes" })
  );
  await screen.findByText("PID 4099");
  expect(screen.queryByText("PID 4100")).not.toBeInTheDocument();
  await user.click(
    screen.getByRole("button", { name: "Load more recorded processes" })
  );
  await screen.findByText("PID 4124");
  expect(processes.mock.calls[1]?.[0].cursor).not.toBeNull();
  await user.click(screen.getByRole("button", { name: "Refresh" }));
  await waitFor(() => expect(processes).toHaveBeenCalledTimes(3));
  expect(processes.mock.calls[2]?.[0].cursor).toBeNull();
  await screen.findByText("PID 4099");
  expect(screen.queryByText("PID 4124")).not.toBeInTheDocument();
});

it("keeps a fixed answer cached across live ticks and exposes partial resource coverage to keyboard users", async () => {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  const read = transport.queryHistoricalUsage.bind(transport);
  const apps = vi
    .spyOn(transport, "queryHistoricalUsage")
    .mockImplementation(async (request) => {
      const result = await read(request);
      if (result.status === "ok")
        for (const app of result.data.apps) {
          const gpu = app.figures.coverage.find(
            (resource) => resource.metric === "gpu"
          );
          if (gpu)
            gpu.coverage = {
              supported: true,
              complete: false,
              measured_ms: 10_000,
              causes: ["read_failed"],
            };
        }
      return result;
    });
  const { user } = renderWithProviders(
    <HistoricalUsageCard range={RANGE} by="cpu" />,
    { transport }
  );
  const cell = await screen.findAllByRole("button", {
    name: /gpu:.*incomplete observation/,
  });
  expect(cell[0]).toHaveTextContent("partial");
  act(() => cell[0]?.focus());
  expect(await screen.findByRole("tooltip")).toHaveTextContent(
    "incomplete observation"
  );
  await user.keyboard("{Escape}");
  await act(() => transport.tick());
  expect(apps).toHaveBeenCalledTimes(1);
});

it("explains intervals beyond retained query support without truncating or querying", async () => {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  const apps = vi.spyOn(transport, "queryHistoricalUsage");
  renderWithProviders(
    <HistoricalUsageCard
      range={{ fromMs: NOW - 90 * 86_400_000, toMs: NOW }}
      by="cpu"
    />,
    { transport }
  );
  await screen.findByText(/supports intervals up to 30 days/);
  expect(apps).not.toHaveBeenCalled();
});

it("retries a source error without offering to reset remote history", async () => {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  const read = transport.queryHistoricalUsage.bind(transport);
  vi.spyOn(transport, "queryHistoricalUsage")
    .mockResolvedValueOnce({
      status: "error",
      error: { kind: "remote_unreachable", message: "disconnected" },
    })
    .mockImplementation(read);
  const { user } = renderWithProviders(
    <HistoricalUsageCard range={RANGE} by="cpu" />,
    { transport }
  );
  await screen.findByRole("alert");
  expect(
    screen.queryByRole("button", { name: /reset/i })
  ).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Retry" }));
  await screen.findByText("Compiler");
});

it("ignores a late answer after the selected interval changes", async () => {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  const read = transport.queryHistoricalUsage.bind(transport);
  let release: (() => void) | undefined;
  vi.spyOn(transport, "queryHistoricalUsage").mockImplementation(
    async (request) => {
      const result = await read(request);
      if (request.to_ms === RANGE.toMs) {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
        if (result.status === "ok" && result.data.apps[0])
          result.data.apps[0].name = "Old selection";
      }
      return result;
    }
  );
  function Investigation() {
    const [range, setRange] = useState(RANGE);
    return (
      <>
        <button
          type="button"
          onClick={() =>
            setRange({
              fromMs: RANGE.fromMs - 600_000,
              toMs: RANGE.toMs - 600_000,
            })
          }
        >
          Earlier selection
        </button>
        <HistoricalUsageCard range={range} by="cpu" />
      </>
    );
  }
  const { user } = renderWithProviders(<Investigation />, { transport });
  await waitFor(() => expect(release).toBeDefined());
  await user.click(screen.getByRole("button", { name: "Earlier selection" }));
  await screen.findByText("Compiler");
  await act(async () => release?.());
  expect(screen.queryByText("Old selection")).not.toBeInTheDocument();
  expect(screen.getByText("Compiler")).toBeInTheDocument();
});

it("restarts a stale process continuation instead of retaining earlier pages", async () => {
  const { user, processes } = setup();
  await screen.findByText("Compiler");
  await user.click(
    screen.getByRole("button", { name: "Show Compiler recorded processes" })
  );
  await screen.findByText("PID 4099");
  processes.mockResolvedValueOnce({
    status: "error",
    error: {
      kind: "invalid_argument",
      message: "process history changed; restart expansion",
    },
  });
  await user.click(
    screen.getByRole("button", { name: "Load more recorded processes" })
  );
  await screen.findByRole("alert");
  await user.click(screen.getByRole("button", { name: "Reload processes" }));
  await screen.findByText("PID 4099");
  expect(processes.mock.calls.at(-1)?.[0].cursor).toBeNull();
  expect(screen.queryByText("PID 4124")).not.toBeInTheDocument();
});

it("shows unrecorded history without inventing empty zero-valued app rows", async () => {
  const transport = createMockTransport({ now: () => NOW, autoTick: false });
  const apps = vi.spyOn(transport, "queryHistoricalUsage");
  renderWithProviders(
    <HistoricalUsageCard
      range={{
        fromMs: NOW - 40 * 86_400_000,
        toMs: NOW - 40 * 86_400_000 + 600_000,
      }}
      by="cpu"
      compare
    />,
    { transport }
  );
  await screen.findByText("Attribution was not recorded for this interval.");
  expect(
    screen.queryByRole("table", { name: "Recorded apps in this range" })
  ).not.toBeInTheDocument();
  expect(screen.queryByText("Compiler")).not.toBeInTheDocument();
  expect(apps).toHaveBeenCalledTimes(1);
});
