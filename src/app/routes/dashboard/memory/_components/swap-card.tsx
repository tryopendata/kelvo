import { seriesRange } from "@core/chart-math";
import { fixed, formatBytes, MISSING } from "@core/format";
import { windowWords } from "@core/live-window";
import { useAreaBrush } from "~/components/area-brush";
import { brushScopeProps } from "~/components/brush-overlay";
import { ChartHistoryStatus } from "~/components/chart-history-status";
import { SectionCard } from "~/components/section-card";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useModuleSeries } from "~/hooks/use-module-series";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useHeld } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { useModuleRange } from "~/stores/chart-range";
import { ceilingAxis, historyTicks } from "~/widgets/lib/chart-labels";
import { StatStrip } from "~/widgets/stat-strip";
import { StreamArea } from "~/widgets/stream-area";

const KEY = "mem.swap_used";
const HEIGHT = 110;

const pages = (v: number | null | undefined) =>
  v == null ? MISSING : `${fixed(v, 0)}`;

/**
 * Swap used over the chart window (D-091) from the ring, with swap-in and
 * swap-out rates. Brushable like the pressure chart (D-099).
 */
export function SwapCard({ windowMs }: { windowMs: number }) {
  const units = useUnits();
  const v = useHeld([KEY, "mem.swap_in", "mem.swap_out"]);
  const endMs = useModuleRange()?.endMs ?? null;
  const span =
    endMs === null ? `last ${windowWords(windowMs)}` : "selected dates";
  const series = useModuleSeries([KEY], windowMs, { brush: true });
  const gaps = useGapBands("memory");
  const brush = useAreaBrush(series, HEIGHT, true, gaps);
  const values = series.values[KEY] ?? [];
  const ceiling = useNiceCeiling(values, series.tEndMs, 100e6, windowMs);
  const fmt = (x: number | null | undefined) =>
    formatBytes(x, { units: units.bytes });

  return (
    <div {...brushScopeProps} className="contents">
      <SectionCard
        accent="mem"
        origin="br"
        variant="default"
        title={`Swap, ${span}`}
        compactHeader
        className="gap-3"
      >
        <StatStrip
          items={[
            { label: "Used", value: fmt(v[KEY]) },
            { label: "In", value: pages(v["mem.swap_in"]), unit: "pages/s" },
            { label: "Out", value: pages(v["mem.swap_out"]), unit: "pages/s" },
          ]}
        />
        <ChartHistoryStatus loading={series.loading} error={series.error} />
        <StreamArea
          gaps={gaps}
          series={[{ key: KEY, values, step: 1 }]}
          tEndMs={series.tEndMs}
          intervalMs={series.intervalMs}
          yMax={ceiling}
          accent="mem"
          height={HEIGHT}
          ariaLabel={`Swap used, ${span}, now ${fmt(v[KEY])}. Drag to select a range.`}
          ceilingLabel={fmt(ceiling)}
          {...ceilingAxis(ceiling)}
          xTicks={historyTicks(
            seriesRange(series).fromMs,
            seriesRange(series).toMs,
            endMs === null,
            3
          )}
          highlight={brush.highlight}
          overlay={brush.overlay}
        />
      </SectionCard>
    </div>
  );
}
