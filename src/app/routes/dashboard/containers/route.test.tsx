import { createMockTransport } from "@core/mock-transport";
import { act, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { expect, it, vi } from "vitest";
import ContainersRoute from "./route";

it("shows current chooser values instead of measurements embedded in retained metadata", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const subscribe = transport.dockerSubscribe;
  transport.dockerSubscribe = (callback) =>
    subscribe((snapshot) =>
      callback({
        ...snapshot,
        containers: snapshot.containers.map((row) => ({
          ...row,
          metrics: { ...row.metrics, cpu_pct: 987 },
        })),
      })
    );
  const result = renderWithProviders(<ContainersRoute />, { transport });
  await result.user.click(
    await screen.findByRole("button", { name: /Choose…|\+\d+ more/ })
  );
  const dialog = screen.getByRole("dialog", { name: "Show on chart" });
  const api = within(dialog)
    .getByRole("checkbox", { name: "Chart api" })
    .closest("label");
  expect(api).toHaveTextContent("987%");
  const removed = within(dialog)
    .getByRole("checkbox", { name: /Chart retired-worker/ })
    .closest("label");
  expect(removed).toHaveTextContent("—");
  result.unmount();
  transport.dispose();
});

it("uses authoritative project totals when retained live rows are stale", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const subscribe = transport.dockerSubscribe;
  transport.dockerSubscribe = (callback) =>
    subscribe((snapshot) =>
      callback({
        ...snapshot,
        project_aggregates: [
          {
            project: "liveoak",
            aggregate: {
              ...snapshot.aggregate,
              cpu_cores: 0.25,
              working_set_bytes: 1024,
              cpu_measured: 1,
              memory_measured: 1,
              expected: 3,
              running: 3,
              total: 3,
              partial: true,
            },
          },
        ],
      })
    );
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?project=liveoak",
  });
  expect(await screen.findByText("CPU 25% (0.3 cores)")).toBeVisible();
  expect(screen.getByText("3 running · 3 total · partial")).toBeVisible();
  result.unmount();
  transport.dispose();
});

it("keeps a custom line and chooser entry outside the bounded ranking across metric changes", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const read = transport.dockerComparison;
  transport.dockerComparison = async (request) => {
    const result = await read(request);
    return result.status === "ok"
      ? {
          ...result,
          data: {
            ...result.data,
            ranking: result.data.ranking.filter(
              (row) => row.container.id !== "container-0"
            ),
          },
        }
      : result;
  };
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?chart=container-0",
  });
  await waitFor(() =>
    expect(
      screen.getByRole("img", { name: /^CPU by container/ })
    ).toHaveAttribute("aria-label", expect.stringContaining("api"))
  );
  await result.user.click(screen.getByRole("radio", { name: "Memory" }));
  await waitFor(() =>
    expect(
      screen.getByRole("img", { name: /^Memory by container/ })
    ).toHaveAttribute("aria-label", expect.stringContaining("api"))
  );
  await result.user.click(
    screen.getByRole("button", { name: /Choose…|\+\d+ more/ })
  );
  expect(screen.getByRole("checkbox", { name: "Chart api" })).toBeChecked();
  result.unmount();
  transport.dispose();
});

it.each([null, "No running containers. Monitoring paused to save power."])(
  "offers Resume for an idle snapshot with message %s",
  async (message) => {
    const transport = createMockTransport({
      scenarios: ["docker-idle"],
      historyRows: 1,
    });
    const subscribe = transport.dockerSubscribe;
    transport.dockerSubscribe = (cb) =>
      subscribe((snapshot) => cb({ ...snapshot, message }));
    const result = renderWithProviders(<ContainersRoute />, { transport });
    expect(await screen.findByRole("button", { name: "Resume" })).toBeVisible();
    expect(screen.getByText("0 running · 0 total")).toBeVisible();
    result.unmount();
    transport.dispose();
  }
);
it("uses one comparison read, updates publication range, and keeps healthy status copy quiet", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const subscribe = transport.dockerSubscribe;
  let now = 12_000_000;
  transport.dockerSubscribe = (cb) =>
    subscribe((snapshot) =>
      cb({ ...snapshot, now_ms: now, observed_ms: now, interval_ms: 0 })
    );
  const read = vi.spyOn(transport, "dockerComparison");
  const history = vi.spyOn(transport, "dockerHistory");
  const result = renderWithProviders(<ContainersRoute />, { transport });
  await screen.findByRole("table", { name: "Containers now" });
  await waitFor(() => expect(read).toHaveBeenCalled());
  expect(history).not.toHaveBeenCalled();
  expect(
    screen.queryByText(/Coverage gap:|Order paused|Workload review/)
  ).toBeNull();
  expect(screen.queryByRole("button", { name: /Details/ })).toBeNull();
  now += 10_000;
  await act(async () => transport.tick());
  await waitFor(() => expect(read.mock.calls.at(-1)?.[0].to_ms).toBe(now));
  result.unmount();
  transport.dispose();
});
it("keeps chart choices separate from automatic engine monitoring", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const configure = vi.spyOn(transport, "dockerConfigure");
  const result = renderWithProviders(<ContainersRoute />, { transport });
  await screen.findByRole("table", { name: "Containers now" });
  await result.user.click(
    await screen.findByRole("button", { name: /Choose…|\+\d+ more/ })
  );
  await result.user.click(
    (
      await screen.findAllByRole("checkbox", { name: /Chart/ })
    )[0] as HTMLElement
  );
  expect(configure).not.toHaveBeenCalled();
  expect(result.router.state.location.search).toContain("chart=");
  await result.user.click(screen.getByRole("button", { name: "Done" }));
  await result.user.click(screen.getByRole("button", { name: "Monitoring…" }));
  expect(screen.queryByRole("radio", { name: "Choose containers" })).toBeNull();
  expect(screen.queryByRole("checkbox", { name: /^Monitor / })).toBeNull();
  expect(
    screen.getByRole("heading", { name: "Automatic collection" })
  ).toBeVisible();
  expect(configure).not.toHaveBeenCalled();
  await result.user.click(screen.getByRole("button", { name: "Done" }));
  expect(configure).not.toHaveBeenCalled();
  result.unmount();
  transport.dispose();
});

it("expands an affected container's group when clearing hidden attention filters", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const subscribe = transport.dockerSubscribe;
  transport.dockerSubscribe = (callback) =>
    subscribe((snapshot) =>
      callback({
        ...snapshot,
        containers: snapshot.containers.map((row) =>
          row.name === "api" ? { ...row, health: "unhealthy" } : row
        ),
      })
    );
  const result = renderWithProviders(<ContainersRoute />, {
    transport,
    route: "/dashboard/containers?q=not-found&group=grouped&collapsed=liveoak",
  });
  await result.user.click(
    await screen.findByRole("button", { name: "1 need attention" })
  );
  expect(
    await screen.findByRole("link", { name: "api · liveoak" })
  ).toBeVisible();
  expect(result.router.state.location.search).not.toContain(
    "collapsed=liveoak"
  );
  result.unmount();
  transport.dispose();
});

it("does not draw previous-engine history or reuse its selection while the next engine loads", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-11"],
    historyRows: 1,
  });
  const subscribe = transport.dockerSubscribe;
  let emit: Parameters<typeof transport.dockerSubscribe>[0] | undefined;
  let snapshot:
    | Parameters<Parameters<typeof transport.dockerSubscribe>[0]>[0]
    | undefined;
  transport.dockerSubscribe = (callback) => {
    emit = callback;
    return subscribe((value) => {
      snapshot = value;
      callback(value);
    });
  };
  const read = transport.dockerComparison;
  const requests: Parameters<typeof read>[0][] = [];
  transport.dockerComparison = (request) => {
    requests.push(request);
    return request.engine_id === "next-engine"
      ? new Promise(() => {})
      : read(request);
  };
  const result = renderWithProviders(<ContainersRoute />, { transport });
  await waitFor(() =>
    expect(
      screen.getByRole("img", { name: /^CPU by container/ })
    ).toHaveAttribute("aria-label", expect.stringContaining("api"))
  );
  if (!snapshot?.engine) throw new Error("Expected initial engine");
  const next = {
    ...snapshot,
    engine: { ...snapshot.engine, id: "next-engine" },
    generation: snapshot.generation + 1,
    containers: snapshot.containers.map((row) => ({
      ...row,
      engine_id: "next-engine",
      name: `next-${row.name}`,
    })),
  };
  await act(async () => emit?.(next));
  expect(
    screen.getByRole("img", { name: /^CPU by container/ })
  ).toHaveAttribute("aria-label", expect.stringContaining("no containers"));
  const request = requests.find((value) => value.engine_id === "next-engine");
  expect(request?.selected).toBeNull();
  result.unmount();
  transport.dispose();
});
