import { stackSum } from "@core/chart-math";
import { formatBytes } from "@core/format";
import { windowWords } from "@core/live-window";
import { useAreaBrush } from "~/components/area-brush";
import { brushScopeProps } from "~/components/brush-overlay";
import { SectionCard } from "~/components/section-card";
import { SelectionSummary } from "~/components/selection-summary";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useHeld } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { useWindowSeries } from "~/hooks/use-window-series";
import { Legend } from "~/widgets/legend";
import { ceilingAxis, windowTicks } from "~/widgets/lib/chart-labels";
import { StreamArea } from "~/widgets/stream-area";

const HEIGHT = 160;
const USED = "mem.used";
const CACHED = "mem.cached";
const KEYS = [USED, CACHED];

/**
 * Cached stacked on used, slot by slot from the newest (the chart
 * right-aligns). A slot missing either part is a gap, never a partial top.
 */
function stackOnto(
  base: readonly (number | null)[],
  top: readonly (number | null)[]
): (number | null)[] {
  const n = Math.min(base.length, top.length);
  const b = base.length - n;
  const t = top.length - n;
  return Array.from({ length: n }, (_, i) =>
    stackSum([base[b + i], top[t + i]])
  );
}

/**
 * Used memory with cached files stacked on it over the chart window, up to
 * MemTotal (D-091), on a host that reports Linux's used, cached and free
 * (D-116) in place of macOS pressure. The PSI figures sit in the composition
 * strip. Brushable like the pressure chart (D-099): a range scopes the
 * peaks and the apps table.
 */
export function UsedCard({
  windowMs,
  totalBytes,
}: {
  windowMs: number;
  /** `HostInfo.mem_total_bytes`: the chart's ceiling. */
  totalBytes: number;
}) {
  const units = useUnits();
  const v = useHeld(KEYS);
  const series = useWindowSeries(KEYS, windowMs, { brush: true });
  const brush = useAreaBrush(series, HEIGHT);
  const gaps = useGapBands("memory");
  const used = series.values[USED] ?? [];
  const fmt = (x: number | null | undefined) =>
    formatBytes(x, { units: units.bytes });

  return (
    <div {...brushScopeProps} className="contents">
      <SectionCard
        accent="mem"
        origin="bl"
        title="Used"
        headerAlign="baseline"
        compactHeader
        headerWrap={false}
        className="col-span-2 gap-3"
        aside={
          <Legend
            items={[
              { label: "Used", value: fmt(v[USED]), step: 1 },
              { label: "Cached files", value: fmt(v[CACHED]), step: 4 },
            ]}
          />
        }
      >
        <div className="flex flex-col gap-1.5">
          <StreamArea
            gaps={gaps}
            series={[
              {
                key: CACHED,
                values: stackOnto(used, series.values[CACHED] ?? []),
                step: 4,
              },
              { key: USED, values: used, step: 1 },
            ]}
            tEndMs={series.tEndMs}
            intervalMs={series.intervalMs}
            yMax={totalBytes}
            accent="mem"
            height={HEIGHT}
            ariaLabel={`Memory used and cached files, last ${windowWords(windowMs)}, of ${fmt(totalBytes)}, used ${fmt(v[USED])}, cached ${fmt(v[CACHED])}. Drag to select a range.`}
            ceilingLabel={fmt(totalBytes)}
            {...ceilingAxis(totalBytes)}
            xTicks={windowTicks(windowMs, 5)}
            highlight={brush.highlight}
            overlay={brush.overlay}
          />
          <SelectionSummary windowMs={windowMs} inset={0} />
        </div>
      </SectionCard>
    </div>
  );
}
