import {
  type Membership,
  nextMembership,
  rankCandidates,
} from "@core/chart-math/series-lines";
import {
  dockerPersistenceLabel,
  dockerRuntimeScopeLabel,
} from "@core/docker-labels";
import { formatBytes, formatPercent } from "@core/format";
import type { DockerSettings } from "@core/generated/bindings";
import { DropdownMenu } from "radix-ui";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocation, useSearchParams } from "react-router";
import { brushScopeProps } from "~/components/brush-overlay";
import { PageHeader } from "~/components/page-header";
import { SearchField } from "~/components/search-field";
import { SectionCard } from "~/components/section-card";
import { SegmentedControl } from "~/components/segmented-control";
import { Button } from "~/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { useUnits } from "~/hooks/use-units";
import { useTransport } from "~/lib/transport-context";
import {
  BrushProvider,
  useBrushRange,
  useBrushStore,
} from "~/stores/brush-store";
import { ChartChooser } from "./_components/chart-chooser";
import {
  ComparisonChart,
  type ComparisonSeries,
} from "./_components/comparison-chart";
import { ComparisonControls } from "./_components/comparison-controls";
import { ContainerActionProvider } from "./_components/container-actions";
import { ContainerTable, needsAttention } from "./_components/container-table";
import { DockerCleanupFlow } from "./_components/docker-cleanup-flow";
import { DockerLocalOnly } from "./_components/docker-local-only";
import { DockerStorage } from "./_components/docker-storage";
import {
  DOCKER_RANGES,
  type DockerRange,
} from "./_components/history-controls";
import { HistoryNotice } from "./_components/history-notice";
import { MonitoringDialog } from "./_components/monitoring-dialog";
import { ProjectScope } from "./_components/project-scope";
import { RetainedHistory } from "./_components/retained-history";
import { useDocker } from "./_hooks/use-docker";
import { useDockerCleanup } from "./_hooks/use-docker-cleanup";
import { useDockerComparison } from "./_hooks/use-docker-comparison";
import {
  type ContainerSort,
  dockerErrorMessage,
  filterContainers,
  lifecycleLabel,
  statusLabel,
} from "./_lib/presentation";

export default function ContainersRoute() {
  return (
    <DockerLocalOnly>
      <BrushProvider>
        <LocalContainersRoute />
      </BrushProvider>
    </DockerLocalOnly>
  );
}
function LocalContainersRoute() {
  const transport = useTransport();
  const { snapshot, error, setSnapshot } = useDocker();
  const { bytes } = useUnits();
  const [monitoring, setMonitoring] = useState(false);
  const [chooser, setChooser] = useState(false);
  const [liveReset, setLiveReset] = useState(0);
  const [actionError, setActionError] = useState<string | null>(null);
  const [params, setParams] = useSearchParams();
  const location = useLocation();
  const brush = useBrushStore();
  const selection = useBrushRange();
  const storageView = params.get("view") === "storage";
  const connected =
    snapshot?.status === "connected" || snapshot?.status === "idle_suspended";
  const engine = snapshot?.engine?.id ?? "";
  const cleanup = useDockerCleanup(
    engine,
    snapshot?.generation ?? 0,
    connected
  );
  const metric = params.get("metric") === "memory" ? "memory" : "cpu";
  const project = params.get("project");
  const query = params.get("q") ?? "";
  const state = params.get("state") ?? "all";
  const sortOverride = useRef<ContainerSort | null>(null);
  const sort = (params.get("sort") ??
    sortOverride.current ??
    metric) as ContainerSort;
  const grouped = params.get("group") === "grouped";
  const columns = (params.get("columns") ?? "").split(",").filter(Boolean);
  const collapsed = params.getAll("collapsed");
  const rawRange = (params.get("range") ?? "15m") as DockerRange;
  const range = rawRange in DOCKER_RANGES ? rawRange : "15m";
  const endValue = params.has("end") ? Number(params.get("end")) : null;
  const end = endValue !== null && Number.isFinite(endValue) ? endValue : null;
  const now = Math.floor((snapshot?.now_ms ?? Date.now()) / 10_000) * 10_000;
  const to = end ?? now;
  const from = to - DOCKER_RANGES[range];
  const custom = params.has("chart")
    ? [
        ...new Set((params.get("chart") ?? "").split(",").filter(Boolean)),
      ].slice(0, 6)
    : null;
  const [focused, setFocused] = useState<string | null>(null);
  const [pinnedFocus, setPinnedFocus] = useState<string | null>(null);
  const [membership, setMembership] = useState<Membership & { engine: string }>(
    {
      engine,
      slots: {},
      streaks: {},
    }
  );
  const currentMembership: Membership =
    membership.engine === engine ? membership : { slots: {}, streaks: {} };
  const focusEngine = useRef(engine);
  useEffect(() => {
    if (focusEngine.current === engine) return;
    focusEngine.current = engine;
    setFocused(null);
    setPinnedFocus(null);
  }, [engine]);
  const selected =
    custom ??
    (Object.keys(currentMembership.slots).length
      ? Object.keys(currentMembership.slots).sort(
          (a, b) =>
            (currentMembership.slots[a] ?? 0) -
            (currentMembership.slots[b] ?? 0)
        )
      : null);
  const comparison = useDockerComparison(
    {
      engine_id: engine,
      from_ms: selection?.fromMs ?? from,
      to_ms: selection?.toMs ?? to,
      metric,
      project,
      selected,
      max_points: 360,
    },
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
    () =>
      rankCandidates(
        (ranking ?? [])
          .map((row) => ({
            id: row.container.id,
            value:
              metric === "cpu" ? row.peak_cpu_pct : row.peak_working_set_bytes,
            container: row.container,
          }))
          .filter(
            (row) =>
              row.value !== null &&
              (project === null || (row.container.project ?? "") === project)
          )
      ),
    [ranking, metric, project]
  );
  const evaluation = useRef("");
  const scope = `${engine}:${metric}:${range}:${end}:${project}`;
  const previousScope = useRef("");
  useEffect(() => {
    if (custom !== null || !ranking || comparison.isPlaceholderData) return;
    const key = `${scope}:${Math.floor((snapshot?.observed_ms ?? 0) / 10_000)}`;
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
    ranking,
    snapshot?.observed_ms,
    focused,
    pinnedFocus,
    comparison.isPlaceholderData,
  ]);
  const slotCache = useRef<Record<string, number>>({});
  const slotEngine = useRef(engine);
  if (slotEngine.current !== engine) {
    slotCache.current = {};
    slotEngine.current = engine;
  }
  if (custom !== null) {
    const retained = Object.fromEntries(
      Object.entries(slotCache.current).filter(([id]) => custom.includes(id))
    );
    for (const id of custom)
      if (!(id in retained))
        retained[id] =
          [1, 2, 3, 4, 5, 6].find(
            (slot) => !Object.values(retained).includes(slot)
          ) ?? 6;
    slotCache.current = retained;
  } else slotCache.current = currentMembership.slots;
  const slots = slotCache.current;
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
    comparison.isPlaceholderData && previousPaint.current.scope === scope
      ? previousPaint.current.series
      : series;
  if (!comparison.isPlaceholderData) previousPaint.current = { scope, series };
  const set = useCallback(
    (key: string, value: string | null) =>
      setParams(
        (old) => {
          const next = new URLSearchParams(old);
          if (value === null) next.delete(key);
          else next.set(key, value);
          next.delete("page");
          return next;
        },
        { replace: key !== "chart" }
      ),
    [setParams]
  );
  useEffect(() => {
    const scroll = (location.state as { scroll?: number } | null)?.scroll;
    if (scroll !== undefined)
      requestAnimationFrame(() => {
        const main = document.querySelector("main");
        if (main) main.scrollTop = scroll;
      });
  }, [location.state]);
  const settings = snapshot?.settings;
  const configure = useCallback(
    async (patch: Partial<DockerSettings>): Promise<boolean> => {
      if (!settings) return false;
      try {
        const result = await transport.dockerConfigure({
          ...settings,
          ...patch,
        });
        if (result.status === "ok") {
          setSnapshot(result.data);
          if (!result.data.engine && patch.enabled) setMonitoring(true);
          return true;
        }
        setActionError(dockerErrorMessage(result.error));
      } catch (e) {
        setActionError(String(e));
      }
      return false;
    },
    [settings, transport, setSnapshot]
  );
  const resume = () => {
    void transport
      .dockerResume()
      .then((result) => {
        if (result.status === "ok") setSnapshot(result.data);
        else setActionError(dockerErrorMessage(result.error));
      })
      .catch((e: unknown) => setActionError(String(e)));
  };
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
    [brush, set]
  );
  if (!snapshot)
    return (
      <div className="flex flex-col gap-3">
        <PageHeader title="Containers" />
        <p
          role={error ? "alert" : "status"}
          className="text-[13px] text-muted-foreground"
        >
          {error ?? "Loading Docker monitoring…"}
        </p>
      </div>
    );
  const scopeRows = snapshot.containers.filter(
    (row) => project === null || (row.project ?? "") === project
  );
  const aggregate =
    project === null
      ? snapshot.aggregate
      : snapshot.project_aggregates.find(
          (row) => row.project === (project || null)
        )?.aggregate;
  const cpu = aggregate?.cpu_cores == null ? null : aggregate.cpu_cores * 100;
  const memory = aggregate?.working_set_bytes ?? null;
  const partial = aggregate?.partial ?? false;
  const historical = end !== null || selection !== null;
  const tableRows = historical
    ? (ranking ?? []).map((r) => ({
        ...r.container,
        lifecycle: r.last_lifecycle ?? ("unknown" as const),
        metrics: {
          ...r.container.metrics,
          cpu_pct: r.peak_cpu_pct,
          working_set_bytes: r.peak_working_set_bytes,
        },
      }))
    : scopeRows;
  const shown = filterContainers(tableRows, query, state);
  const hiddenAttention = tableRows.filter(
    (row) =>
      needsAttention(row) && !shown.some((visible) => visible.id === row.id)
  );
  const projectNames = [
    ...new Set(snapshot.containers.map((row) => row.project ?? "")),
  ].sort();
  if (project !== null && !projectNames.includes(project))
    projectNames.push(project);
  const filters = (
    <>
      <SearchField
        value={query}
        onChange={(value) => set("q", value || null)}
        placeholder="Search containers…"
        label="Search containers by name, image or project"
      />
      <Select
        value={state}
        onValueChange={(value) => set("state", value === "all" ? null : value)}
      >
        <SelectTrigger aria-label="Container state" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All states</SelectItem>
          {Object.entries(lifecycleLabel).map(([value, label]) => (
            <SelectItem key={value} value={value}>
              {label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <Button variant="outline" size="sm">
            View
          </Button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className="z-50 min-w-40 rounded-control border border-border bg-popover p-2 text-[12px] shadow-md">
            <DropdownMenu.CheckboxItem
              checked={grouped}
              onCheckedChange={(value) =>
                set("group", value ? "grouped" : null)
              }
              className="cursor-default rounded-control px-2 py-2 focus:bg-selected"
            >
              {grouped ? "✓ " : ""}Group by project
            </DropdownMenu.CheckboxItem>
            {[
              ["io", "Block I/O"],
              ["image", "Image"],
              ["ports", "Ports"],
              ["restarts", "Restarts"],
              ["pids", "PIDs"],
            ].map(([value, label]) => (
              <DropdownMenu.CheckboxItem
                key={value}
                checked={columns.includes(value ?? "")}
                onCheckedChange={(checked) =>
                  set(
                    "columns",
                    (checked
                      ? [...columns, value]
                      : columns.filter((c) => c !== value)
                    ).join(",") || null
                  )
                }
                className="cursor-default rounded-control px-2 py-2 focus:bg-selected"
              >
                {columns.includes(value ?? "") ? "✓ " : ""}
                {label}
              </DropdownMenu.CheckboxItem>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </>
  );
  const historyControls = (
    <ComparisonControls
      metric={metric}
      range={range}
      end={end}
      now={now}
      onMetric={setMetric}
      onRange={setRange}
      onEnd={setEnd}
    />
  );
  return (
    <ContainerActionProvider snapshot={snapshot}>
      <div className="flex min-w-0 flex-col gap-3" {...brushScopeProps}>
        <PageHeader
          title="Containers"
          subtitle={`${snapshot.engine?.runtime ?? "Local Docker"} · ${statusLabel[snapshot.status]}`}
          actions={
            <Button
              size="sm"
              variant="outline"
              onClick={() => setMonitoring(true)}
            >
              Monitoring…
            </Button>
          }
        />
        {(actionError || error) && (
          <p role="alert" className="text-[12px] text-destructive">
            {actionError ?? error}
          </p>
        )}
        {monitoring && (
          <MonitoringDialog
            snapshot={snapshot}
            onChange={setSnapshot}
            configure={configure}
            onClose={() => setMonitoring(false)}
          />
        )}
        {!snapshot.settings.enabled ? (
          <>
            <section className="flex flex-wrap items-center justify-between gap-3 py-2">
              <p className="max-w-xl text-[13px] text-muted-foreground">
                Monitor CPU, memory and lifecycle evidence for one local Docker
                engine. Saved history remains available below.
              </p>
              <Button onClick={() => void configure({ enabled: true })}>
                Enable monitoring
              </Button>
            </section>
            <RetainedHistory enabled={false} generation={snapshot.generation} />
          </>
        ) : (
          <>
            <SegmentedControl
              options={[
                { value: "activity", label: "Activity" },
                { value: "storage", label: "Storage & cleanup" },
              ]}
              value={storageView ? "storage" : "activity"}
              onChange={(value) => set("view", value)}
              ariaLabel="Containers view"
            />
            <DockerCleanupFlow flow={cleanup} />
            {storageView ? (
              <DockerStorage
                key={`${engine}:${snapshot.generation}`}
                engine={engine}
                generation={snapshot.generation}
                connected={connected}
                flow={cleanup}
              />
            ) : (
              <>
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="figures flex flex-wrap items-baseline gap-x-5 gap-y-1 text-[13px]">
                    {project !== null && <span>{project || "Standalone"}</span>}
                    <span>
                      CPU {formatPercent(cpu)} (
                      {cpu === null ? "—" : (cpu / 100).toFixed(1)} cores)
                    </span>
                    <span>Memory {formatBytes(memory, { units: bytes })}</span>
                    <span>
                      {aggregate?.running ?? 0} running ·{" "}
                      {aggregate?.total ?? 0} total{partial ? " · partial" : ""}
                    </span>
                  </div>
                  <ProjectScope
                    projects={projectNames}
                    value={project}
                    onChange={setProject}
                  />
                </div>
                <ComparisonChart
                  key={`${engine}:${metric}:${liveReset}`}
                  series={plottedSeries}
                  metric={metric}
                  from={from}
                  to={to}
                  live={end === null && !selection}
                  range={range}
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
                      <p
                        role="alert"
                        className="text-[11px] text-muted-foreground"
                      >
                        History could not be read: {comparison.error.message}
                      </p>
                    ) : (
                      <HistoryNotice
                        snapshot={snapshot}
                        persistence={comparisonPage?.persistence}
                        coverage={comparisonPage?.coverage ?? []}
                        metric={metric}
                        onManage={() => setMonitoring(true)}
                        onResume={resume}
                      />
                    )
                  }
                />
                <ChartChooser
                  open={chooser}
                  onOpenChange={setChooser}
                  candidates={
                    chooser
                      ? [
                          ...new Map([
                            ...candidates.map(
                              (row) => [row.id, row.container] as const
                            ),
                            ...series.map(
                              (line) =>
                                [line.container.id, line.container] as const
                            ),
                          ]).values(),
                        ]
                      : []
                  }
                  liveRows={snapshot.containers}
                  selected={selected ?? []}
                  slots={slots}
                  metric={metric}
                  onChange={(ids) => {
                    const nextSlots = Object.fromEntries(
                      Object.entries(slots).filter(([id]) => ids.includes(id))
                    );
                    for (const id of ids)
                      if (!(id in nextSlots))
                        nextSlots[id] =
                          [1, 2, 3, 4, 5, 6].find(
                            (slot) => !Object.values(nextSlots).includes(slot)
                          ) ?? 6;
                    set(
                      "chart",
                      [...ids]
                        .sort(
                          (a, b) => (nextSlots[a] ?? 0) - (nextSlots[b] ?? 0)
                        )
                        .join(",")
                    );
                  }}
                  onAutomatic={() => {
                    setMembership({ engine, slots, streaks: {} });
                    evaluation.current = "";
                    previousScope.current = "";
                    set("chart", null);
                  }}
                />
                <SectionCard
                  accent="cpu"
                  title="Containers now"
                  hiddenTitle
                  flush
                >
                  <ContainerTable
                    liveRows={snapshot.containers}
                    rows={shown}
                    sort={sort}
                    onSort={(value) => {
                      sortOverride.current = value;
                      set("sort", value);
                    }}
                    grouped={grouped}
                    columns={columns}
                    collapsed={collapsed}
                    onCollapse={(group) =>
                      setParams(
                        (old) => {
                          const next = new URLSearchParams(old);
                          const values = next.getAll("collapsed");
                          next.delete("collapsed");
                          for (const v of values.includes(group)
                            ? values.filter((v) => v !== group)
                            : [...values, group])
                            next.append("collapsed", v);
                          return next;
                        },
                        { replace: true }
                      )
                    }
                    slots={slots}
                    focused={pinnedFocus ?? focused}
                    onFocus={setFocused}
                    filters={filters}
                    historical={historical}
                    title={
                      historical
                        ? `Containers · ${new Date(selection?.fromMs ?? from).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}–${new Date(selection?.toMs ?? to).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
                        : "Containers now"
                    }
                    attention={
                      hiddenAttention.length ? (
                        <button
                          type="button"
                          className="text-[11px] underline"
                          onClick={() => {
                            setParams(
                              (old) => {
                                const next = new URLSearchParams(old);
                                next.delete("q");
                                next.delete("state");
                                const targetGroup =
                                  hiddenAttention[0]?.project ?? "Standalone";
                                const hiddenGroups = next
                                  .getAll("collapsed")
                                  .filter((group) => group !== targetGroup);
                                next.delete("collapsed");
                                for (const group of hiddenGroups)
                                  next.append("collapsed", group);
                                return next;
                              },
                              { replace: true }
                            );
                            setFocused(hiddenAttention[0]?.id ?? null);
                          }}
                        >
                          {hiddenAttention.length} need attention
                        </button>
                      ) : undefined
                    }
                  />
                </SectionCard>
                <details className="border-border-subtle border-y py-3 text-[12px]">
                  <summary className="cursor-pointer focus-visible:ring-2 focus-visible:ring-ring">
                    Runtime &amp; history
                  </summary>
                  <div className="mt-3 space-y-3">
                    <div>
                      <p>
                        Runtime processes on this Mac{" "}
                        <span className="figures ml-3">
                          CPU {formatPercent(snapshot.runtime.cpu_pct)} ·
                          Footprint{" "}
                          {formatBytes(snapshot.runtime.footprint_bytes)}
                        </span>{" "}
                        ·{" "}
                        {dockerRuntimeScopeLabel[snapshot.runtime.scope] ??
                          snapshot.runtime.scope}
                      </p>
                      <p className="mt-2 text-muted-foreground">
                        {snapshot.runtime.explanation}
                        {snapshot.engine?.capacity_observed_ms != null
                          ? ` Engine capacity observed ${new Date(snapshot.engine.capacity_observed_ms).toLocaleString()}.`
                          : " Engine capacity measurement time unavailable."}
                      </p>
                      <p className="mt-1 text-muted-foreground">
                        {snapshot.runtime.processes.join(" · ") ||
                          "No verified runtime processes"}
                        {snapshot.runtime.observed_ms !== null
                          ? ` · observed ${new Date(snapshot.runtime.observed_ms).toLocaleTimeString()}`
                          : ""}
                      </p>
                    </div>
                    <p className="text-[11px] text-muted-foreground">
                      History:{" "}
                      {dockerPersistenceLabel[snapshot.persistence.status] ??
                        snapshot.persistence.status}{" "}
                      · {formatBytes(snapshot.persistence.logical_bytes)} /{" "}
                      {formatBytes(snapshot.persistence.allowance_bytes)}{" "}
                      logical allowance
                      {snapshot.persistence.retained_from_ms !== null
                        ? ` · retained from ${new Date(snapshot.persistence.retained_from_ms).toLocaleString()}`
                        : ""}
                    </p>
                    <RetainedHistory enabled generation={snapshot.generation} />
                  </div>
                </details>
              </>
            )}
          </>
        )}
      </div>
    </ContainerActionProvider>
  );
}
