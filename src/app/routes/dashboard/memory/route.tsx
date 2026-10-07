import { PageHeader } from "~/components/page-header";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useHostRecord } from "~/hooks/use-host-record";
import { CompositionCard } from "./_components/composition-card";
import { MemoryProcessesCard } from "./_components/memory-processes-card";
import { PressureCard } from "./_components/pressure-card";
import { SwapCard } from "./_components/swap-card";
import { marketingGb } from "./_lib/pressure";

/**
 * Memory page (plan 4.9): the Overview and popover Memory card pieces in the
 * CPU page's layout. Composition leads; pressure over time and swap sit below it.
 * The cards wait for the chart window (D-091), so they appear together.
 */
export default function MemoryRoute() {
  const info = useHostRecord()?.info;
  const totalGb = info ? marketingGb(info.mem_total_bytes) : null;
  const windowMs = useChartWindow()?.windowMs ?? null;

  return (
    <div className="flex flex-col gap-4">
      <PageHeader
        title="Memory"
        subtitle={
          totalGb !== null && (
            <>
              <span className="data-mono">{totalGb}</span> GB unified
            </>
          )
        }
        actions={<WindowControl />}
      />
      {windowMs !== null && (
        <>
          <CompositionCard
            totalGb={totalGb}
            totalBytes={info?.mem_total_bytes ?? null}
          />
          <div className="grid grid-cols-[repeat(auto-fit,minmax(320px,1fr))] gap-4">
            <PressureCard windowMs={windowMs} />
            <SwapCard windowMs={windowMs} />
          </div>
          <MemoryProcessesCard />
        </>
      )}
    </div>
  );
}
