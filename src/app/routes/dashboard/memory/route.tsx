import { formatBytes, MISSING } from "@core/format";
import { useMemo } from "react";
import { PageHeader } from "~/components/page-header";
import {
  UsageAppsCard,
  type UsageTableConfig,
} from "~/components/usage-apps-card";
import { WindowControl } from "~/components/window-control";
import { useChartWindow } from "~/hooks/use-chart-window";
import { useHostRecord } from "~/hooks/use-host-record";
import { useUnits } from "~/hooks/use-units";
import { BrushProvider } from "~/stores/brush-store";
import { CompositionCard } from "./_components/composition-card";
import { PressureCard } from "./_components/pressure-card";
import { SwapCard } from "./_components/swap-card";
import { marketingGb } from "./_lib/pressure";

/**
 * Memory page (plan 4.9): the Overview and popover Memory card pieces in the
 * CPU page's layout. Composition leads; pressure over time and swap sit below it.
 * The cards wait for the chart window (D-091), so they appear together. A
 * range brushed on either chart scopes the peaks and the apps table (D-099).
 */
export default function MemoryRoute() {
  const info = useHostRecord()?.info;
  const totalGb = info ? marketingGb(info.mem_total_bytes) : null;
  const windowMs = useChartWindow()?.windowMs ?? null;
  const bytes = useUnits().bytes;
  const table = useMemo((): UsageTableConfig => {
    const fmt = (n: number | undefined) =>
      n === undefined ? MISSING : formatBytes(n, { units: bytes });
    return {
      by: "memory",
      noun: "Memory",
      accent: "mem",
      columns: [
        { label: "Peak", format: (u) => fmt(u.mem_peak_bytes), ranked: true },
        { label: "Avg while running", format: (u) => fmt(u.mem_avg_bytes) },
      ],
      footnote: (
        <>
          Footprint, as Activity Monitor's Memory column counts it. An app's
          peak is its processes summed at one sample, so the processes under it
          can add up to more. The average is over the time the app was running.
          Other users' and macOS processes aren't listed.
        </>
      ),
    };
  }, [bytes]);

  return (
    <BrushProvider>
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
              windowMs={windowMs}
            />
            <div className="grid grid-cols-[repeat(auto-fit,minmax(320px,1fr))] gap-4">
              <PressureCard windowMs={windowMs} />
              <SwapCard windowMs={windowMs} />
            </div>
            <UsageAppsCard windowMs={windowMs} config={table} />
          </>
        )}
      </div>
    </BrushProvider>
  );
}
