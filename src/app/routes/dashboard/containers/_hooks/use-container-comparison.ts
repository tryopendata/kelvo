import {
  type Membership,
  nextMembership,
  rankCandidates,
} from "@core/chart-math/series-lines";
import type {
  DockerComparisonPage,
  DockerComparisonRequest,
  DockerRankedContainer,
  DockerSnapshot,
} from "@core/generated/bindings";
import {
  type Dispatch,
  type SetStateAction,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useBrushRange, useBrushStore } from "~/stores/brush-store";
import type { ComparisonSeries } from "../_components/comparison-chart";
import type { ContainerRouteState } from "../_lib/route-state";
import { useDockerComparison } from "./use-docker-comparison";

export function useContainerComparison(
  snapshot: DockerSnapshot | null,
  route: ContainerRouteState
) {
  const { metric, project, range, end, from, to, custom } = route;
  const engine = snapshot?.engine?.id ?? "";
  const selection = useBrushRange();
  const { focused, setFocused, pinnedFocus, setPinnedFocus } =
    useContainerFocus(engine);
  const [membership, setMembership] = useState<Membership & { engine: string }>(
    {
      engine,
      slots: {},
      streaks: {},
    }
  );
  const currentMembership: Membership =
    membership.engine === engine ? membership : { slots: {}, streaks: {} };
  const selected =
    custom ??
    (Object.keys(currentMembership.slots).length
      ? Object.keys(currentMembership.slots).sort(
          (a, b) =>
            (currentMembership.slots[a] ?? 0) -
            (currentMembership.slots[b] ?? 0)
        )
      : null);
  const request: DockerComparisonRequest = {
    engine_id: engine,
    from_ms: from,
    to_ms: to,
    metric,
    project,
    selected,
    max_points: 360,
  };
  const comparison = useDockerComparison(request, snapshot?.generation ?? 0);
  // A brush scopes the table, never the plotted window. Identical unselected
  // requests share the query cache, so live viewing still makes one read.
  const selectedComparison = useDockerComparison(
    selection
      ? { ...request, from_ms: selection.fromMs, to_ms: selection.toMs }
      : request,
    snapshot?.generation ?? 0
  );
  // Query placeholders belong to their original engine, even if container IDs match.
  const comparisonPage =
    comparison.data?.ranking.some(
      (row) => row.container.engine_id !== engine
    ) ||
    comparison.data?.series.some((line) => line.container.engine_id !== engine)
      ? undefined
      : comparison.data;
  const ranking = comparisonPage?.ranking;
  const candidates = useMemo(
    () => comparisonCandidates(ranking, metric, project),
    [ranking, metric, project]
  );
  const scope = `${engine}:${metric}:${range}:${end}:${project}`;
  const resetEvaluation = useMembershipEvaluation({
    engine,
    scope,
    custom,
    hasRanking: ranking !== undefined,
    observedMs: snapshot?.observed_ms,
    placeholder: comparison.isPlaceholderData,
    candidates,
    focused,
    pinnedFocus,
    setMembership,
  });
  const { slots, series, plottedSeries } = useComparisonPaint({
    engine,
    custom,
    currentMembership,
    comparisonPage,
    project,
    scope,
    placeholder: comparison.isPlaceholderData,
  });
  const automatic = () => {
    setMembership({ engine, slots, streaks: {} });
    resetEvaluation();
  };
  return {
    focused,
    setFocused,
    pinnedFocus,
    setPinnedFocus,
    selected,
    comparison,
    selectedComparison,
    comparisonPage,
    ranking,
    candidates,
    slots,
    series,
    plottedSeries,
    automatic,
  };
}

/** Custom membership keeps stable color slots while adding/removing identities. */
export function retainSlots(
  previous: Record<string, number>,
  ids: readonly string[]
) {
  const retained = Object.fromEntries(
    Object.entries(previous).filter(([id]) => ids.includes(id))
  );
  for (const id of ids) {
    if (!Object.hasOwn(retained, id))
      retained[id] =
        [1, 2, 3, 4, 5, 6].find(
          (slot) => !Object.values(retained).includes(slot)
        ) ?? 6;
  }
  return retained;
}

function useComparisonPaint({
  engine,
  custom,
  currentMembership,
  comparisonPage,
  project,
  scope,
  placeholder,
}: {
  engine: string;
  custom: string[] | null;
  currentMembership: Membership;
  comparisonPage: DockerComparisonPage | undefined;
  project: string | null;
  scope: string;
  placeholder: boolean;
}) {
  const slotCache = useRef<Record<string, number>>({});
  const slotEngine = useRef(engine);
  const priorSlots = slotEngine.current === engine ? slotCache.current : {};
  const slots =
    custom === null ? currentMembership.slots : retainSlots(priorSlots, custom);
  useLayoutEffect(() => {
    slotCache.current = slots;
    slotEngine.current = engine;
  });
  const series = (comparisonPage?.series ?? []).flatMap((line) => {
    const row = line.container;
    return slots[row.id] &&
      (project === null || (row.project ?? "") === project)
      ? [
          {
            container: row,
            slot: slots[row.id] ?? 1,
            points: line.points,
          },
        ]
      : [];
  });
  // A re-ranked selection becomes visible atomically when its comparison read arrives.
  const previousPaint = useRef<{ scope: string; series: ComparisonSeries[] }>({
    scope: "",
    series: [],
  });
  const plottedSeries =
    placeholder && previousPaint.current.scope === scope
      ? previousPaint.current.series
      : series;
  useLayoutEffect(() => {
    if (!placeholder) previousPaint.current = { scope, series };
  });
  return { slots, series, plottedSeries };
}

function comparisonCandidates(
  ranking: readonly DockerRankedContainer[] | undefined,
  metric: "cpu" | "memory",
  project: string | null
) {
  return rankCandidates(
    (ranking ?? [])
      .map((row) => ({
        id: row.container.id,
        value: metric === "cpu" ? row.peak_cpu_pct : row.peak_working_set_bytes,
        container: row.container,
      }))
      .filter(
        (row) =>
          row.value !== null &&
          (project === null || (row.container.project ?? "") === project)
      )
  );
}

function useMembershipEvaluation({
  engine,
  scope,
  custom,
  hasRanking,
  observedMs,
  placeholder,
  candidates,
  focused,
  pinnedFocus,
  setMembership,
}: {
  engine: string;
  scope: string;
  custom: string[] | null;
  hasRanking: boolean;
  observedMs: number | undefined;
  placeholder: boolean;
  candidates: ReturnType<typeof comparisonCandidates>;
  focused: string | null;
  pinnedFocus: string | null;
  setMembership: Dispatch<SetStateAction<Membership & { engine: string }>>;
}) {
  const evaluation = useRef("");
  const previousScope = useRef("");
  useEffect(() => {
    if (custom !== null || !hasRanking || placeholder) return;
    const key = `${scope}:${Math.floor((observedMs ?? 0) / 10_000)}`;
    if (evaluation.current === key) return;
    const immediate = previousScope.current !== scope;
    evaluation.current = key;
    previousScope.current = scope;
    setMembership((old) => ({
      ...nextMembership(
        old.engine === engine ? old : { slots: {}, streaks: {} },
        candidates,
        immediate,
        [pinnedFocus ?? "", focused ?? ""]
      ),
      engine,
    }));
  }, [
    scope,
    engine,
    candidates,
    custom,
    hasRanking,
    observedMs,
    focused,
    pinnedFocus,
    placeholder,
    setMembership,
  ]);
  return () => {
    evaluation.current = "";
    previousScope.current = "";
  };
}

function useContainerFocus(engine: string) {
  const brush = useBrushStore();
  const [focused, setFocused] = useState<string | null>(null);
  const [pinnedFocus, setPinnedFocus] = useState<string | null>(null);
  const focusEngine = useRef(engine);
  useEffect(() => {
    if (focusEngine.current === engine) return;
    focusEngine.current = engine;
    brush.getState().clear();
    setFocused(null);
    setPinnedFocus(null);
  }, [engine, brush]);
  return { focused, setFocused, pinnedFocus, setPinnedFocus };
}
