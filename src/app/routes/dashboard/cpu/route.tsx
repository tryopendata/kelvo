import { PageHeader } from "~/components/page-header";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useHostRecord } from "~/hooks/use-host-record";
import { CoreLoadCard } from "./_components/core-load-card";
import { TopProcessesCard } from "./_components/top-processes-card";
import { TotalCard } from "./_components/total-card";
import { clusterViews, coreCounts } from "./_lib/clusters";

/**
 * CPU page (plan 4.7). The total chart is the page's dominant
 * region. The cards wait for the chart window (D-091), so they appear together.
 */
export default function CpuRoute() {
  const info = useHostRecord()?.info;
  const clusters = clusterViews(info?.cpu_topology ?? []);
  const counts = info ? coreCounts(info) : null;
  const coreCount = counts ? counts.performance + counts.efficiency : null;
  const windowMs = useChartWindow()?.windowMs ?? null;

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="CPU"
        subtitle={
          info &&
          counts && (
            <>
              {info.chip ?? "Unknown chip"} ·{" "}
              <span className="data-mono">{counts.performance}</span>{" "}
              performance +{" "}
              <span className="data-mono">{counts.efficiency}</span> efficiency
              cores
            </>
          )
        }
        actions={<WindowControl />}
      />
      {windowMs !== null && (
        <>
          <TotalCard windowMs={windowMs} />
          <CoreLoadCard clusters={clusters} windowMs={windowMs} />
          <TopProcessesCard coreCount={coreCount || null} />
        </>
      )}
    </div>
  );
}
