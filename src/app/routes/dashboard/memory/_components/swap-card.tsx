import { fixed, formatBytes, MISSING } from "@core/format";
import { windowWords } from "@core/live-window";
import { useId } from "react";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useHeld } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { useWindowSeries } from "~/hooks/use-window-series";
import { Card } from "~/widgets/card";
import { windowTicks } from "~/widgets/lib/chart-labels";
import { StatStrip } from "~/widgets/stat-strip";
import { StreamArea } from "~/widgets/stream-area";

const KEY = "mem.swap_used";

const pages = (v: number | null | undefined) =>
  v == null ? MISSING : `${fixed(v, 0)}`;

/**
 * Swap used over the chart window (D-091) from the ring, with swap-in and
 * swap-out rates.
 */
export function SwapCard({ windowMs }: { windowMs: number }) {
  const titleId = useId();
  const span = `last ${windowWords(windowMs)}`;
  const units = useUnits();
  const v = useHeld([KEY, "mem.swap_in", "mem.swap_out"]);
  const series = useWindowSeries([KEY], windowMs);
  const gaps = useGapBands("memory");
  const values = series.values[KEY] ?? [];
  const ceiling = useNiceCeiling(values, series.tEndMs, 100e6, windowMs);
  const fmt = (x: number | null | undefined) =>
    formatBytes(x, { units: units.bytes });

  return (
    <Card
      accent="mem"
      origin="br"
      labelledBy={titleId}
      className="flex flex-col gap-3 p-4"
    >
      <h2 id={titleId} className="m-0 font-[590] text-[14px]">
        Swap, {span}
      </h2>
      <StatStrip
        items={[
          { label: "Used", value: fmt(v[KEY]) },
          { label: "In", value: pages(v["mem.swap_in"]), unit: "pages/s" },
          { label: "Out", value: pages(v["mem.swap_out"]), unit: "pages/s" },
        ]}
      />
      <StreamArea
        gaps={gaps}
        series={[{ key: KEY, values, step: 1 }]}
        tEndMs={series.tEndMs}
        intervalMs={series.intervalMs}
        yMax={ceiling}
        accent="mem"
        height={110}
        ariaLabel={`Swap used, ${span}, now ${fmt(v[KEY])}`}
        ceilingLabel={fmt(ceiling)}
        gridlines={[ceiling / 2, ceiling]}
        xTicks={windowTicks(windowMs, 3)}
      />
    </Card>
  );
}
