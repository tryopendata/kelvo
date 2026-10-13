import { createMockTransport } from "@core/mock-transport";
import { unwrap } from "@core/transport";
import { afterEach, expect, it, vi } from "vitest";
import { MOCK_DOCKER_ENGINE } from "./docker";

afterEach(() => vi.useRealTimers());
const request = {
  engine_id: MOCK_DOCKER_ENGINE,
  from_ms: 1_790_000_000_000,
  to_ms: 1_790_000_900_000,
  metric: "cpu" as const,
  project: null,
  selected: null,
  max_points: 360,
};

it("provides eleven containers, five measured, and shared realistic history", async () => {
  const t = createMockTransport({ scenarios: ["docker-11"], historyRows: 1 });
  const snapshot = await unwrap(t.dockerSnapshot());
  expect(snapshot.containers).toHaveLength(11);
  expect(snapshot.aggregate.running).toBe(5);
  expect(snapshot.containers.slice(0, 5).map((row) => row.name)).toEqual([
    "api",
    "worker",
    "caddy",
    "grafana-dev",
    "tempo",
  ]);
  const comparison = await unwrap(t.dockerComparison(request));
  expect(comparison.series).toHaveLength(5);
  expect(
    comparison.ranking.filter((row) => row.container.lifecycle === "removed")
  ).toHaveLength(2);
  const first = comparison.series.find(
    (row) => row.container.id === "container-0"
  );
  expect(first).toBeDefined();
  if (!first) throw new Error("No mock series");
  const history = await unwrap(
    t.dockerHistory({ ...request, container_id: first.container.id })
  );
  expect(first.points).toEqual(history.points);
  expect(first.points.some((p) => p.cpu_measured_ms === 9999)).toBe(true);
  expect(first.points.filter((p) => !p.cpu_covered)).toHaveLength(2);
  expect(
    first.points.some(
      (p, i) =>
        i > 0 &&
        p.time_ms - (first.points[i - 1]?.time_ms ?? p.time_ms) > 10_000
    )
  ).toBe(true);
  t.dispose();
});

it("bounds comparison requests and retains removed history only in the first half", async () => {
  const t = createMockTransport({ scenarios: ["docker-11"], historyRows: 1 });
  expect(
    (
      await t.dockerComparison({
        ...request,
        selected: Array.from({ length: 7 }, (_, i) => `container-${i}`),
      })
    ).status
  ).toBe("error");
  expect(
    (await t.dockerComparison({ ...request, max_points: 361 })).status
  ).toBe("error");
  const page = await unwrap(
    t.dockerComparison({
      ...request,
      project: "liveoak",
      selected: ["removed-spike"],
    })
  );
  expect(page.ranking.every((row) => row.container.project === "liveoak")).toBe(
    true
  );
  expect(page.series).toHaveLength(1);
  expect(
    page.series[0]?.points.every((p) => p.time_ms < request.from_ms + 450_000)
  ).toBe(true);
  const rolled = await unwrap(
    t.dockerComparison({
      ...request,
      from_ms: request.to_ms - 86_400_000,
      max_points: 360,
    })
  );
  expect(
    rolled.series.some((row) =>
      row.points.some(
        (p) => (p.peak.cpu_pct ?? 0) === (p.metrics.cpu_pct ?? 0) * 3
      )
    )
  ).toBe(true);
  t.dispose();
});

it("serializes row and storage operations, changes lifecycle after 500 ms, and preserves failure messages", async () => {
  vi.useFakeTimers();
  const t = createMockTransport({ scenarios: ["docker-11"], historyRows: 1 });
  const op = await unwrap(
    t.dockerContainerAction(MOCK_DOCKER_ENGINE, "container-0", "stop")
  );
  expect(op.finished_ms).toBeNull();
  expect(
    (await t.dockerContainerAction(MOCK_DOCKER_ENGINE, "container-1", "stop"))
      .status
  ).toBe("error");
  expect(
    (
      await t.dockerStoragePrepare([
        { kind: "volume", id: "old-database-data", action: "delete" },
      ])
    ).status
  ).toBe("error");
  await vi.advanceTimersByTimeAsync(500);
  expect((await unwrap(t.dockerSnapshot())).containers[0]?.lifecycle).toBe(
    "exited"
  );
  const start = await unwrap(
    t.dockerContainerAction(MOCK_DOCKER_ENGINE, "container-0", "start")
  );
  await vi.advanceTimersByTimeAsync(500);
  expect(
    (await unwrap(t.dockerStorageOperation(start.id))).results[0]?.status
  ).toBe("succeeded");
  await t.dockerContainerAction(MOCK_DOCKER_ENGINE, "container-1", "stop");
  await vi.advanceTimersByTimeAsync(500);
  const worker = await unwrap(
    t.dockerContainerAction(MOCK_DOCKER_ENGINE, "container-1", "start")
  );
  await vi.advanceTimersByTimeAsync(500);
  expect(
    (await unwrap(t.dockerStorageOperation(worker.id))).results[0]
  ).toMatchObject({ status: "failed", message: "Port 8080 is already in use" });
  t.dispose();
});

it("keeps collection and container state when only display preferences change", async () => {
  vi.useFakeTimers();
  const t = createMockTransport({ scenarios: ["docker-11"], historyRows: 1 });
  await t.dockerContainerAction(MOCK_DOCKER_ENGINE, "container-0", "stop");
  await vi.advanceTimersByTimeAsync(500);
  const before = await unwrap(t.dockerSnapshot());
  const settings = {
    ...before.settings,
    show_popover: !before.settings.show_popover,
    show_tray_count: !before.settings.show_tray_count,
  };
  const after = await unwrap(t.dockerConfigure(settings));
  expect(after.generation).toBe(before.generation);
  expect(after.containers).toEqual(before.containers);
  expect(after.aggregate).toEqual(before.aggregate);
  expect(after.settings).toEqual(settings);
  expect((await t.getSettings()).settings.docker).toEqual(settings);
  t.dispose();
});
