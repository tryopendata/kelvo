import { formatPercent } from "@core/format";
import type { ProcessView } from "@core/process-interest";
import { PageHeader } from "~/components/page-header";
import {
  UsageAppsCard,
  type UsageTableConfig,
} from "~/components/usage-apps-card";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useHostRecord } from "~/hooks/use-host-record";
import {
  useProcessGpu,
  useProcessInterest,
} from "~/hooks/use-process-interest";
import { BrushProvider } from "~/stores/brush-store";
import { FrequencyCard } from "./_components/frequency-card";
import { PowerCard } from "./_components/power-card";
import { UsageCard } from "./_components/usage-card";

/**
 * GPU time on the idle cadence's process samples, no rows: in Performance
 * mode GPU per process is measured only while a view asks, so the table has
 * figures for the time this page is open (D-085, D-099). Outside it the
 * engine measures GPU anyway and this adds nothing.
 */
const GPU_VIEW: ProcessView = {
  limit: 0,
  sort: ["gpu"],
  period_ms: 10_000,
  gpu: true,
};

const TABLE: UsageTableConfig = {
  by: "gpu",
  noun: "GPU",
  accent: "gpu",
  columns: [
    {
      label: "Avg GPU",
      format: (u) => formatPercent(u.gpu_avg_pct, { decimals: 1 }),
      ranked: true,
    },
  ],
  footnote: (
    <>
      Average share of the whole GPU over the time it was measured. Approximate
      while a long GPU compute job runs: its time is counted when each batch of
      GPU work finishes. System and other is the rest of GPU utilization:
      WindowServer and other users' processes.
    </>
  ),
};

/**
 * GPU page (plan 4.8): the CPU page's layout with the Overview GPU card
 * pieces. The usage chart leads; frequency and power sit below it. A range
 * brushed on the usage chart scopes its average and peak and, with
 * per-process GPU time (D-085), the apps table (D-099); without it the table
 * is not there. The cards wait for the chart window (D-091), so they appear
 * together.
 */
export default function GpuRoute() {
  const info = useHostRecord()?.info;
  const perProcess = useProcessGpu();
  useProcessInterest(perProcess ? GPU_VIEW : null);
  const windowMs = useChartWindow()?.windowMs ?? null;
  return (
    <BrushProvider>
      <div className="flex flex-col gap-4">
        <PageHeader
          title="GPU"
          subtitle={info?.chip ?? undefined}
          actions={<WindowControl />}
        />
        {windowMs !== null && (
          <>
            <UsageCard windowMs={windowMs} />
            <div className="grid grid-cols-[repeat(auto-fit,minmax(320px,1fr))] gap-4">
              <FrequencyCard windowMs={windowMs} />
              <PowerCard windowMs={windowMs} />
            </div>
            {perProcess && <UsageAppsCard windowMs={windowMs} config={TABLE} />}
          </>
        )}
      </div>
    </BrushProvider>
  );
}
