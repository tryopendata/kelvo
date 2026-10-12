import {
  dockerPersistenceLabel,
  dockerRuntimeScopeLabel,
} from "@core/docker-labels";
import { formatBytes, formatPercent } from "@core/format";
import { dockerKeys } from "@core/query-keys";
import { useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useLocation, useSearchParams } from "react-router";
import { PageHeader } from "~/components/page-header";
import { SearchField } from "~/components/search-field";
import { SectionCard } from "~/components/section-card";
import { SegmentedControl } from "~/components/segmented-control";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { useTransport } from "~/lib/transport-context";
import { ContainerHistory } from "./_components/container-history";
import { ContainerTable } from "./_components/container-table";
import { DockerActivityReview } from "./_components/docker-activity-review";
import { DockerCleanupFlow } from "./_components/docker-cleanup-flow";
import { DockerLocalOnly } from "./_components/docker-local-only";
import { DockerStorage } from "./_components/docker-storage";
import { EngineChooser } from "./_components/engine-chooser";
import {
  DOCKER_RANGES,
  type DockerRange,
  HistoryControls,
} from "./_components/history-controls";
import { RetainedHistory } from "./_components/retained-history";
import {
  useDocker,
  useDockerEvents,
  useDockerHistory,
} from "./_hooks/use-docker";
import { useDockerCleanup } from "./_hooks/use-docker-cleanup";
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
      <LocalContainersRoute />
    </DockerLocalOnly>
  );
}

function LocalContainersRoute() {
  const transport = useTransport();
  const { snapshot, error, setSnapshot } = useDocker();
  const [chooser, setChooser] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [params, setParams] = useSearchParams();
  const location = useLocation();
  const client = useQueryClient();
  const storageView = params.get("view") === "storage";
  const cleanupConnected =
    snapshot?.status === "connected" || snapshot?.status === "idle_suspended";
  const cleanup = useDockerCleanup(
    snapshot?.engine?.id ?? "",
    snapshot?.generation ?? 0,
    cleanupConnected
  );
  const query = params.get("q") ?? "";
  const state = params.get("state") ?? "all";
  const sort = (params.get("sort") ?? "cpu") as ContainerSort;
  const grouped = params.get("group") !== "flat";
  const io = params.get("columns") === "io";
  const collapsed = useMemo(() => params.getAll("collapsed"), [params]);
  const range = (params.get("range") ?? "15m") as DockerRange;
  const validRange = range in DOCKER_RANGES ? range : "15m";
  const end = params.has("end") ? Number(params.get("end")) : null;
  const now = Math.floor((snapshot?.now_ms ?? Date.now()) / 10_000) * 10_000;
  const to = end ?? now;
  const from = to - DOCKER_RANGES[validRange];
  const engine = snapshot?.engine?.id ?? "";
  const history = useDockerHistory(
    engine,
    null,
    from,
    to,
    snapshot?.generation
  );
  const events = useDockerEvents(engine, null, from, to, snapshot?.generation);
  const [metric, setMetric] = useState<"cpu" | "memory">("cpu");
  const set = useCallback(
    (key: string, value: string | null) =>
      setParams(
        (previous) => {
          const next = new URLSearchParams(previous);
          if (value === null) next.delete(key);
          else next.set(key, value);
          return next;
        },
        { replace: true }
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
  const currentSettings = snapshot?.settings;
  const configure = useCallback(
    async (patch: Partial<NonNullable<typeof snapshot>["settings"]>) => {
      if (!currentSettings) return;
      try {
        const result = await transport.dockerConfigure({
          ...currentSettings,
          ...patch,
        });
        if (result.status === "ok") {
          setSnapshot(result.data);
          if (result.data.engine === null && patch.enabled) setChooser(true);
        } else setActionError(dockerErrorMessage(result.error));
      } catch (e) {
        setActionError(String(e));
      }
    },
    [currentSettings, transport, setSnapshot]
  );
  const rows = snapshot?.containers;
  const shown = useMemo(
    () => filterContainers(rows ?? [], query, state),
    [rows, query, state]
  );
  const monitor = useCallback(
    (id: string, monitored: boolean) => {
      const ids = (rows ?? [])
        .filter((row) => row.monitored && row.id !== id)
        .map((row) => row.id);
      if (monitored) ids.push(id);
      void configure({ monitored_containers: ids, explicit_selection: true });
    },
    [rows, configure]
  );
  const filters = useMemo(
    () => (
      <div className="flex flex-wrap items-center gap-3">
        <SearchField
          value={query}
          onChange={(q) => set("q", q)}
          placeholder="Name, image or project"
          label="Search containers by name, image or project"
        />
        <Select value={state} onValueChange={(value) => set("state", value)}>
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
        <SegmentedControl
          options={[
            { value: "grouped", label: "Grouped" },
            { value: "flat", label: "Flat" },
          ]}
          value={grouped ? "grouped" : "flat"}
          onChange={(value) => set("group", value)}
          ariaLabel="Container grouping"
        />
        <SegmentedControl
          options={[
            { value: "cpu", label: "CPU" },
            { value: "io", label: "I/O" },
          ]}
          value={io ? "io" : "cpu"}
          onChange={(value) => set("columns", value)}
          ariaLabel="Container columns"
        />
      </div>
    ),
    [query, state, grouped, io, set]
  );
  const historyControls = useMemo(
    () => (
      <HistoryControls
        range={validRange}
        end={end}
        onRange={(r) => set("range", r)}
        onEnd={(value) => set("end", value === null ? null : String(value))}
        now={now}
      />
    ),
    [validRange, end, now, set]
  );
  const metricControl = useMemo(
    () => (
      <SegmentedControl
        options={[
          { value: "cpu", label: "CPU" },
          { value: "memory", label: "Memory" },
        ]}
        value={metric}
        onChange={setMetric}
        ariaLabel="Workload chart"
      />
    ),
    [metric]
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
  const aggregate = snapshot.aggregate;
  return (
    <div className="flex flex-col gap-3">
      <PageHeader
        title="Containers"
        subtitle={`${snapshot.engine?.runtime ?? "Local Docker"} · ${statusLabel[snapshot.status]}`}
        actions={
          <Button
            size="sm"
            variant="outline"
            onClick={() => setChooser((value) => !value)}
          >
            Engine…
          </Button>
        }
      />
      {(actionError || error) && (
        <p role="alert" className="text-[12px] text-destructive">
          {actionError ?? error}
        </p>
      )}
      {chooser && (
        <EngineChooser
          settings={snapshot.settings}
          onChange={setSnapshot}
          onClose={() => setChooser(false)}
        />
      )}
      {!snapshot.settings.enabled ? (
        <>
          <section className="flex flex-wrap items-center justify-between gap-3 py-2">
            <p className="max-w-xl text-[13px] text-muted-foreground">
              Monitor CPU, memory and lifecycle evidence for one local Docker
              engine. Collection is read-only; saved history remains available
              below.
            </p>
            <Button onClick={() => void configure({ enabled: true })}>
              Enable monitoring
            </Button>
          </section>
          <RetainedHistory enabled={false} generation={snapshot.generation} />
        </>
      ) : (
        <>
          {(snapshot.message || snapshot.status === "idle_suspended") && (
            <p
              role="status"
              className="rounded-control border border-border px-3 py-2 text-[12px] text-muted-foreground"
            >
              {snapshot.message}
              {snapshot.status === "idle_suspended" && (
                <Button
                  size="sm"
                  variant="outline"
                  className="ml-3"
                  onClick={() =>
                    void transport.dockerResume().then((r) => {
                      if (r.status === "ok") {
                        setSnapshot(r.data);
                        void client.invalidateQueries({
                          queryKey: dockerKeys.storage(
                            r.data.engine?.id ?? "",
                            r.data.generation
                          ),
                        });
                      } else setActionError(dockerErrorMessage(r.error));
                    })
                  }
                >
                  Resume monitoring
                </Button>
              )}
            </p>
          )}
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
              connected={cleanupConnected}
              flow={cleanup}
            />
          ) : (
            <>
              <div className="flex flex-wrap items-end justify-between gap-3">
                <div className="figures flex flex-wrap items-baseline gap-x-6 gap-y-2">
                  <span className="text-[13px]">
                    {aggregate.running} running / {aggregate.total} total
                  </span>
                  <span
                    title="Occupied vCPU cores. One core equals 100% in container rows."
                    className="figures-display text-[26px]"
                  >
                    {aggregate.cpu_cores?.toFixed(2) ?? "—"}
                    <span className="ml-1 font-normal text-[12px] text-muted-foreground">
                      cores
                    </span>
                  </span>
                  <span title="Summed container working set against independently observed engine capacity, not Mac RAM.">
                    {formatBytes(aggregate.working_set_bytes)} /{" "}
                    {formatBytes(aggregate.capacity_bytes)}
                  </span>
                </div>
                {historyControls}
              </div>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="figures text-[11px] text-muted-foreground">
                  <span role="status">
                    Container workload · CPU {aggregate.cpu_measured}/
                    {aggregate.expected}, memory {aggregate.memory_measured}/
                    {aggregate.expected} measured
                    {aggregate.partial
                      ? " · Partial population; totals do not identify the global top consumer"
                      : ""}
                  </span>
                  <span>
                    {" · observed "}
                    {new Date(snapshot.observed_ms).toLocaleTimeString()}
                    {snapshot.interval_ms > 0
                      ? ` · every ${snapshot.interval_ms / 1000}s`
                      : ""}
                  </span>
                </div>
                {metricControl}
              </div>
              {history.error ? (
                <p role="alert" className="text-[12px] text-muted-foreground">
                  History could not be read: {history.error.message}
                </p>
              ) : (
                <ContainerHistory
                  aggregate
                  compact
                  title={
                    end === null
                      ? "Container workload"
                      : "Historical container workload · table below is Now"
                  }
                  page={history.data}
                  metric={metric === "cpu" ? "cpu_pct" : "working_set_bytes"}
                  from={from}
                  to={to}
                  events={events.data?.events}
                />
              )}
              <details className="border-border-subtle border-y py-3 text-[12px]">
                <summary className="cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring">
                  Runtime processes on this Mac{" "}
                  <span className="figures ml-3">
                    CPU {formatPercent(snapshot.runtime.cpu_pct)} · Footprint{" "}
                    {formatBytes(snapshot.runtime.footprint_bytes)}
                  </span>{" "}
                  ·{" "}
                  {dockerRuntimeScopeLabel[snapshot.runtime.scope] ??
                    snapshot.runtime.scope}
                </summary>
                <p className="mt-2 text-muted-foreground">
                  {snapshot.runtime.explanation}
                  {snapshot.engine?.capacity_observed_ms !== null &&
                  snapshot.engine?.capacity_observed_ms !== undefined
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
              </details>
              <DockerActivityReview
                key={`${engine}:${snapshot.generation}`}
                rows={snapshot.containers}
                connected={cleanupConnected}
                flow={cleanup}
              />
              {filters}{" "}
              <SectionCard
                accent="cpu"
                title="Containers now"
                hiddenTitle
                flush
              >
                <ContainerTable
                  rows={shown}
                  page={Number(params.get("page") ?? 0)}
                  onPage={(page) => set("page", String(page))}
                  sort={sort}
                  onSort={(value) => set("sort", value)}
                  monitoringFull={
                    snapshot.aggregate.monitored >=
                    snapshot.aggregate.monitoring_limit
                  }
                  onMonitored={monitor}
                  grouped={grouped}
                  io={io}
                  collapsed={collapsed}
                  onCollapse={(group) =>
                    setParams(
                      (previous) => {
                        const next = new URLSearchParams(previous);
                        const values = next.getAll("collapsed");
                        next.delete("collapsed");
                        for (const value of values.includes(group)
                          ? values.filter((v) => v !== group)
                          : [...values, group])
                          next.append("collapsed", value);
                        return next;
                      },
                      { replace: true }
                    )
                  }
                />
              </SectionCard>
              <footer className="flex flex-wrap items-center gap-4 text-[11px] text-muted-foreground">
                <span>
                  History:{" "}
                  {dockerPersistenceLabel[snapshot.persistence.status] ??
                    snapshot.persistence.status}{" "}
                  · {formatBytes(snapshot.persistence.logical_bytes)} /{" "}
                  {formatBytes(snapshot.persistence.allowance_bytes)} logical
                  allowance
                  {snapshot.persistence.retained_from_ms !== null
                    ? ` · retained from ${new Date(snapshot.persistence.retained_from_ms).toLocaleString()}`
                    : ""}
                </span>
                <label
                  htmlFor="docker-popover"
                  className="flex items-center gap-1"
                >
                  <Checkbox
                    id="docker-popover"
                    checked={snapshot.settings.show_popover}
                    onCheckedChange={(checked) =>
                      void configure({ show_popover: checked === true })
                    }
                  />
                  Popover summary
                </label>
                <label
                  htmlFor="docker-tray"
                  className="flex items-center gap-1"
                >
                  <Checkbox
                    id="docker-tray"
                    checked={snapshot.settings.show_tray_count}
                    onCheckedChange={(checked) =>
                      void configure({ show_tray_count: checked === true })
                    }
                  />
                  Tray count
                </label>
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void configure({ enabled: false })}
                >
                  Disable monitoring
                </Button>
              </footer>
            </>
          )}
        </>
      )}
      {snapshot.settings.enabled && !storageView && (
        <RetainedHistory enabled generation={snapshot.generation} />
      )}
    </div>
  );
}
