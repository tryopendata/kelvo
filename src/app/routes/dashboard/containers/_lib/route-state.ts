import { DOCKER_RANGES, dockerRange, historyEnd } from "./history-params";
import { type ContainerSort, lifecycleLabel } from "./presentation";

const SORTS: readonly ContainerSort[] = [
  "cpu",
  "memory",
  "name",
  "restarts",
  "network",
  "io",
];
const COLUMNS = ["io", "image", "ports", "restarts", "pids"];

export function containerRouteState(
  params: URLSearchParams,
  nowMs: number,
  fallbackSort: ContainerSort | null
) {
  const metric: "cpu" | "memory" =
    params.get("metric") === "memory" ? "memory" : "cpu";
  const range = dockerRange(params.get("range"), "15m");
  const end = historyEnd(params.get("end"));
  const now = Math.floor(nowMs / 10_000) * 10_000;
  const to = end ?? now;
  const rawSort = params.get("sort");
  const state = params.get("state") ?? "all";
  return {
    storageView: params.get("view") === "storage",
    metric,
    project: params.get("project"),
    query: params.get("q") ?? "",
    state: Object.hasOwn(lifecycleLabel, state) ? state : "all",
    sort: SORTS.find((sort) => sort === rawSort) ?? fallbackSort ?? metric,
    grouped: params.get("group") === "grouped",
    columns: [
      ...new Set(
        (params.get("columns") ?? "")
          .split(",")
          .filter((column) => COLUMNS.includes(column))
      ),
    ],
    collapsed: params.getAll("collapsed"),
    range,
    end,
    now,
    to,
    from: to - DOCKER_RANGES[range],
    custom: params.has("chart")
      ? [
          ...new Set((params.get("chart") ?? "").split(",").filter(Boolean)),
        ].slice(0, 6)
      : null,
  };
}

export type ContainerRouteState = ReturnType<typeof containerRouteState>;

/** Browser back state is read defensively, independently of route query parsing. */
export function restoredScroll(state: unknown): number | undefined {
  if (typeof state !== "object" || state === null || !("scroll" in state))
    return undefined;
  return typeof state.scroll === "number" &&
    Number.isFinite(state.scroll) &&
    state.scroll >= 0
    ? state.scroll
    : undefined;
}
