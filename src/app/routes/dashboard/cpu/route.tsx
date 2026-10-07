import { formatPercent } from "@core/format";
import { useNavigate } from "react-router";
import { PageHeader } from "~/components/page-header";
import {
  UsageAppsCard,
  type UsageTableConfig,
} from "~/components/usage-apps-card";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useHostRecord } from "~/hooks/use-host-record";
import { BrushProvider } from "~/stores/brush-store";
import { CoreLoadCard } from "./_components/core-load-card";
import { TotalCard } from "./_components/total-card";
import { clusterViews, coreCounts } from "./_lib/clusters";

/**
 * CPU page (plan 4.7). The total chart is the page's dominant
 * region. The cards wait for the chart window (D-091), so they appear
 * together. A range brushed on the total chart scopes its average and peak
 * and the apps table (D-099); the per-core heatmap stays live.
 */
export default function CpuRoute() {
  const info = useHostRecord()?.info;
  const clusters = clusterViews(info?.cpu_topology ?? []);
  const counts = info ? coreCounts(info) : null;
  const coreCount = counts ? counts.performance + counts.efficiency : null;
  const windowMs = useChartWindow()?.windowMs ?? null;

  return (
    <BrushProvider>
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
                <span className="data-mono">{counts.efficiency}</span>{" "}
                efficiency cores
              </>
            )
          }
          actions={<WindowControl />}
        />
        {windowMs !== null && (
          <>
            <TotalCard windowMs={windowMs} />
            <CoreLoadCard clusters={clusters} windowMs={windowMs} />
            <UsageAppsCard
              windowMs={windowMs}
              config={cpuTable(coreCount || null)}
            />
          </>
        )}
      </div>
    </BrushProvider>
  );
}

const cpu = (u: { cpu_avg_pct: number | null }) =>
  formatPercent(u.cpu_avg_pct, { decimals: 1 });

function cpuTable(coreCount: number | null): UsageTableConfig {
  return {
    by: "cpu",
    noun: "CPU",
    accent: "cpu",
    columns: [{ label: "Avg CPU", format: cpu, ranked: true }],
    footnote: <CpuFootnote coreCount={coreCount} />,
  };
}

function CpuFootnote({ coreCount }: { coreCount: number | null }) {
  const navigate = useNavigate();
  return (
    <>
      Average % of one core over the sampled time
      {coreCount !== null && (
        <>
          ; <span className="data-mono">{coreCount}</span> cores ={" "}
          <span className="data-mono">{coreCount * 100}%</span>
        </>
      )}
      . System and other is the rest of the CPU total: other users&apos; and
      macOS processes, and ones that ran for less than a sample.{" "}
      <button
        type="button"
        onClick={() => navigate("/dashboard/processes")}
        className="rounded-sm text-fg-subtle underline-offset-2 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring"
      >
        Every process
      </button>
    </>
  );
}
