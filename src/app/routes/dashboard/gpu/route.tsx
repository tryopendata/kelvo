import { PageHeader } from "~/components/page-header";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useHostRecord } from "~/hooks/use-host-record";
import { useProcessGpu } from "~/hooks/use-process-interest";
import { FrequencyCard } from "./_components/frequency-card";
import { GpuProcessesCard } from "./_components/gpu-processes-card";
import { PowerCard } from "./_components/power-card";
import { UsageCard } from "./_components/usage-card";

/**
 * GPU page (plan 4.8): the CPU page's layout with the Overview GPU card
 * pieces. The usage chart leads; frequency and power sit below it. With
 * per-process GPU time (v1.2, D-085) a process table follows; without it the
 * section is not there. The cards wait for the chart window (D-091), so they
 * appear together.
 */
export default function GpuRoute() {
  const info = useHostRecord()?.info;
  const perProcess = useProcessGpu();
  const windowMs = useChartWindow()?.windowMs ?? null;
  return (
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
          {perProcess && <GpuProcessesCard />}
        </>
      )}
    </div>
  );
}
