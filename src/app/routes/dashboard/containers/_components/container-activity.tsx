import type { DockerSnapshot } from "@core/generated/bindings";
import { windowWords } from "@core/live-window";
import { useCallback, useState } from "react";
import type { SetURLSearchParams } from "react-router";
import { useBrushStore } from "~/stores/brush-store";
import type { useContainerComparison } from "../_hooks/use-container-comparison";
import type { ContainerSort } from "../_lib/presentation";
import type { ContainerRouteState } from "../_lib/route-state";
import { ComparisonChart } from "./comparison-chart";
import { ComparisonControls } from "./comparison-controls";
import { ContainerChartChooser } from "./container-chart-chooser";
import { ContainerResults } from "./container-results";
import { ContainerSummary } from "./container-summary";
import { DockerRuntime } from "./docker-runtime";
import type { DockerRange } from "./history-controls";
import { HistoryNotice } from "./history-notice";
export function ContainerActivity({
  snapshot,
  route,
  chart,
  set,
  setParams,
  onSort,
  onManage,
  resume,
}: {
  snapshot: DockerSnapshot;
  route: ContainerRouteState;
  chart: ReturnType<typeof useContainerComparison>;
  set: (key: string, value: string | null) => void;
  setParams: SetURLSearchParams;
  onSort: (sort: ContainerSort) => void;
  onManage: () => void;
  resume: () => void;
}) {
  const { metric, project, range, end, now, from, to, custom } = route;
  const {
    focused,
    setFocused,
    pinnedFocus,
    setPinnedFocus,
    comparison,
    comparisonPage,
    candidates,
    plottedSeries,
  } = chart;
  const engine = snapshot.engine?.id ?? "";
  const brush = useBrushStore();
  const [chooser, setChooser] = useState(false);
  const [liveReset, setLiveReset] = useState(0);
  const setProject = useCallback(
    (value: string | null) => set("project", value),
    [set]
  );
  const setMetric = useCallback(
    (value: "cpu" | "memory") => set("metric", value),
    [set]
  );
  const setRange = useCallback(
    (value: DockerRange) => {
      brush.getState().clear();
      set("range", value);
    },
    [brush, set]
  );
  const setEnd = useCallback(
    (value: number | null) => {
      brush.getState().clear();
      setPinnedFocus(null);
      setFocused(null);
      setLiveReset((old) => old + 1);
      set("end", value === null ? null : String(value));
    },
    [brush, set, setFocused, setPinnedFocus]
  );
  const historyControls = (
    <ComparisonControls
      metric={metric}
      range={range}
      windowMs={route.windowMs}
      end={end}
      now={now}
      onMetric={setMetric}
      onRange={setRange}
      onEnd={setEnd}
    />
  );
  return (
    <>
      <ContainerSummary
        snapshot={snapshot}
        project={project}
        onProject={setProject}
      />
      <ComparisonChart
        key={`${engine}:${metric}:${liveReset}`}
        series={plottedSeries}
        metric={metric}
        from={from}
        to={to}
        live={end === null}
        range={end === null ? windowWords(route.windowMs) : "selected dates"}
        controls={historyControls}
        coverage={comparisonPage?.coverage}
        loading={comparison.isPending}
        focused={focused}
        pinnedFocus={pinnedFocus}
        onFocus={setFocused}
        onPinFocus={setPinnedFocus}
        onChoose={() => setChooser(true)}
        more={Math.max(0, candidates.length - plottedSeries.length)}
        chooserTitle={
          custom
            ? "Custom selection"
            : candidates.length > 6
              ? `Top 5 by peak ${metric === "cpu" ? "CPU" : "memory"}`
              : undefined
        }
        notice={
          comparison.error ? (
            <p role="alert" className="text-[11px] text-muted-foreground">
              History could not be read: {comparison.error.message}
            </p>
          ) : (
            <HistoryNotice
              snapshot={snapshot}
              persistence={comparisonPage?.persistence}
              coverage={comparisonPage?.coverage ?? []}
              metric={metric}
              onManage={() => onManage()}
              onResume={resume}
            />
          )
        }
      />
      <ContainerChartChooser
        chart={chart}
        snapshot={snapshot}
        metric={metric}
        open={chooser}
        onOpenChange={setChooser}
        set={set}
      />
      <ContainerResults
        snapshot={snapshot}
        route={route}
        chart={chart}
        set={set}
        setParams={setParams}
        onSort={onSort}
      />
      <DockerRuntime snapshot={snapshot} />
    </>
  );
}
