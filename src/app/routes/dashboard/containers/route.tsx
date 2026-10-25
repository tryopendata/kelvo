import type { DockerSettings, DockerSnapshot } from "@core/generated/bindings";
import type { ReactNode } from "react";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { useLocation, useSearchParams } from "react-router";
import { PageHeader } from "~/components/page-header";
import { SegmentedControl } from "~/components/segmented-control";
import { Button } from "~/components/ui/button";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { BrushProvider } from "~/stores/brush-store";
import { useCgroups } from "~/stores/live-selectors";
import { CgroupContainers } from "./_components/cgroup-containers";
import { ContainerActionProvider } from "./_components/container-actions";
import { ContainerActivity } from "./_components/container-activity";
import { DockerCleanupFlow } from "./_components/docker-cleanup-flow";
import { DockerLocalOnly } from "./_components/docker-local-only";
import { DockerStorage } from "./_components/docker-storage";
import { MonitoringDialog } from "./_components/monitoring-dialog";
import { RetainedHistory } from "./_components/retained-history";
import { useContainerComparison } from "./_hooks/use-container-comparison";
import { useDocker } from "./_hooks/use-docker";
import { useDockerCleanup } from "./_hooks/use-docker-cleanup";
import { useDockerConfiguration } from "./_hooks/use-docker-configuration";
import { type ContainerSort, statusLabel } from "./_lib/presentation";
import { containerRouteState, restoredScroll } from "./_lib/route-state";

export default function ContainersRoute() {
  const local = useIsLocalHost();
  const cgroups = useCgroups();
  // A remote Linux host's page reads its cgroup series; the local Docker
  // page keeps its own model (D-116).
  if (!local && cgroups) return <CgroupContainers />;
  return (
    <DockerLocalOnly>
      <BrushProvider>
        <LocalContainersRoute />
      </BrushProvider>
    </DockerLocalOnly>
  );
}
function LocalContainersRoute() {
  const { snapshot, error, setSnapshot } = useDocker();
  const { monitoring, setMonitoring, actionError, configure, resume } =
    useDockerConfiguration(snapshot, setSnapshot);
  const [params, setParams] = useSearchParams();
  const location = useLocation();
  const connected =
    snapshot?.status === "connected" || snapshot?.status === "idle_suspended";
  const engine = snapshot?.engine?.id ?? "";
  const cleanup = useDockerCleanup(
    engine,
    snapshot?.generation ?? 0,
    connected
  );
  const sortOverride = useRef<ContainerSort | null>(null);
  const routeState = containerRouteState(
    params,
    snapshot?.now_ms ?? Date.now(),
    sortOverride.current
  );
  const { storageView } = routeState;
  const chart = useContainerComparison(snapshot, routeState);
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
  const viewControl = useMemo(
    () => (
      <SegmentedControl
        options={[
          { value: "activity", label: "Activity" },
          { value: "storage", label: "Storage & cleanup" },
        ]}
        value={storageView ? "storage" : "activity"}
        onChange={(value) => set("view", value)}
        ariaLabel="Containers view"
      />
    ),
    [storageView, set]
  );
  useRestoredScroll(location.state);
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
  return (
    <ContainerActionProvider snapshot={snapshot}>
      <div className="flex min-w-0 flex-col gap-3">
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
        <MonitoringContent snapshot={snapshot} configure={configure}>
          {viewControl}
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
            <ContainerActivity
              snapshot={snapshot}
              route={routeState}
              chart={chart}
              set={set}
              setParams={setParams}
              onSort={(value) => {
                sortOverride.current = value;
                set("sort", value);
              }}
              onManage={() => setMonitoring(true)}
              resume={resume}
            />
          )}
        </MonitoringContent>
      </div>
    </ContainerActionProvider>
  );
}

function useRestoredScroll(state: unknown) {
  useEffect(() => {
    const scroll = restoredScroll(state);
    if (scroll !== undefined)
      requestAnimationFrame(() => {
        const main = document.querySelector("main");
        if (main) main.scrollTop = scroll;
      });
  }, [state]);
}

function MonitoringContent({
  snapshot,
  configure,
  children,
}: {
  snapshot: DockerSnapshot;
  configure: (patch: Partial<DockerSettings>) => Promise<boolean>;
  children: ReactNode;
}) {
  if (snapshot.settings.enabled) return children;
  return (
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
  );
}
