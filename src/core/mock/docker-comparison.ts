import type {
  DockerComparisonPage,
  DockerComparisonRequest,
  DockerContainer,
  DockerSnapshot,
} from "@core/generated/bindings";
import type { CommandResult } from "@core/transport";
import { MiB } from "./docker-fixtures";
import { mockHistoryPoints } from "./docker-history";
import type { ScenarioName } from "./fixtures";

const ok = <T>(data: T): CommandResult<T> => ({ status: "ok", data });
function comparisonRow(
  container: DockerContainer,
  request: DockerComparisonRequest,
  scenarios: readonly ScenarioName[],
  now: () => number,
  startedAt: number
) {
  const boosted =
    scenarios.includes("docker-rerank") &&
    now() - startedAt >= 15_000 &&
    container.id === "container-4"
      ? {
          ...container,
          metrics: {
            ...container.metrics,
            cpu_pct: 450,
            working_set_bytes: 2_000 * MiB,
          },
        }
      : container;
  const points = mockHistoryPoints(
    { ...request, container_id: container.id },
    boosted
  );
  const cpu = points.filter(
    (p) => p.cpu_measured_ms > 0 && p.metrics.cpu_pct !== null
  );
  const memory = points.filter(
    (p) => p.memory_measured_ms > 0 && p.metrics.working_set_bytes !== null
  );
  const cpuWeight = cpu.reduce((sum, p) => sum + p.cpu_measured_ms, 0);
  const memoryWeight = memory.reduce((sum, p) => sum + p.memory_measured_ms, 0);
  return {
    container,
    points,
    peak_cpu_pct: cpu.length
      ? Math.max(...cpu.map((p) => p.peak.cpu_pct ?? 0))
      : null,
    avg_cpu_pct: cpuWeight
      ? cpu.reduce(
          (sum, p) => sum + (p.metrics.cpu_pct ?? 0) * p.cpu_measured_ms,
          0
        ) / cpuWeight
      : null,
    peak_working_set_bytes: memory.length
      ? Math.max(...memory.map((p) => p.peak.working_set_bytes ?? 0))
      : null,
    avg_working_set_bytes: memoryWeight
      ? memory.reduce(
          (sum, p) =>
            sum + (p.metrics.working_set_bytes ?? 0) * p.memory_measured_ms,
          0
        ) / memoryWeight
      : null,
    cpu_measured_ms: cpuWeight,
    memory_measured_ms: memoryWeight,
    last_lifecycle: container.lifecycle,
  };
}
export function comparison(
  request: DockerComparisonRequest,
  retained: DockerContainer[],
  snapshot: DockerSnapshot,
  scenarios: readonly ScenarioName[],
  now: () => number,
  startedAt: number
): CommandResult<DockerComparisonPage> {
  if (
    request.max_points < 1 ||
    request.max_points > 360 ||
    (request.selected?.length ?? 0) > 6 ||
    request.from_ms >= request.to_ms
  )
    return {
      status: "error",
      error: {
        kind: "invalid_argument",
        message:
          "Comparison requires 1–360 points, at most six containers and a valid range.",
      },
    };
  const candidates = retained.filter(
    (row) =>
      row.engine_id === request.engine_id &&
      (request.project === null || (row.project ?? "") === request.project)
  );
  const all = candidates
    .map((container) => {
      return comparisonRow(container, request, scenarios, now, startedAt);
    })
    .filter((row) =>
      request.metric === "cpu"
        ? row.cpu_measured_ms > 0
        : row.memory_measured_ms > 0
    );
  const value = (row: (typeof all)[number]) =>
    request.metric === "cpu" ? row.peak_cpu_pct : row.peak_working_set_bytes;
  all.sort(
    (a, b) =>
      (value(b) ?? -Infinity) - (value(a) ?? -Infinity) ||
      a.container.id.localeCompare(b.container.id)
  );
  const ranking = all.slice(0, 500);
  const selected =
    request.selected ??
    ranking
      .slice(0, ranking.length <= 6 ? 6 : 5)
      .map((row) => row.container.id);
  const page: DockerComparisonPage = {
    generation: snapshot.generation,
    ranking: ranking.map(({ points: _points, ...row }) => row),
    series: selected.flatMap((id) => {
      const row = all.find((row) => row.container.id === id);
      return row ? [{ container: row.container, points: row.points }] : [];
    }),
    coverage: snapshot.coverage,
    persistence: snapshot.persistence,
  };
  return ok(page);
}
