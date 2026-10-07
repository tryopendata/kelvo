import { ratio } from "@core/chart-math";
import { bytesParts, formatBytes, formatPercent, MISSING } from "@core/format";
import type { MetricStat } from "@core/generated/bindings";
import { useId, useMemo } from "react";
import { brushScopeProps } from "~/components/brush-overlay";
import { RangeTotalsStrip } from "~/components/range-totals-strip";
import { useHeld } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { Card } from "~/widgets/card";
import { StackBar } from "~/widgets/stack-bar";
import { StatStrip } from "~/widgets/stat-strip";
import { pressureState } from "../_lib/pressure";

const HELD = [
  "mem.used",
  "mem.app",
  "mem.wired",
  "mem.compressed",
  "mem.cached",
  "mem.free",
  "mem.pressure",
  "mem.pressure_level",
  "mem.swap_used",
];

/**
 * What memory holds right now: used of total, pressure with its state word,
 * swap and compressed, then the composition bar (the Overview Memory card and
 * the popover bar, at page scale). Beside the live strip, the peaks over the
 * chart window or the brushed range (D-099).
 */
export function CompositionCard({
  totalGb,
  totalBytes,
  windowMs,
}: {
  totalGb: number | null;
  /** `HostInfo.mem_total_bytes`: the bar's denominator, as in the popover (D-092). */
  totalBytes: number | null;
  windowMs: number;
}) {
  const titleId = useId();
  const units = useUnits();
  const v = useHeld(HELD);
  const at = (k: string) => v[k] ?? null;
  const used = at("mem.used");
  // Shares of installed memory; a missing part is its own empty segment.
  const frac = (x: number | null) => ratio(x, totalBytes);
  const fmt = (x: number | null) => formatBytes(x, { units: units.bytes });
  const state = pressureState(at("mem.pressure_level"));
  const totals = useMemo(
    () =>
      [
        { metric: "mem.used", label: "Peak used" },
        { metric: "mem.swap_used", label: "Peak swap" },
      ].map((t) => ({
        ...t,
        format: (s: MetricStat) => formatBytes(s.max, { units: units.bytes }),
      })),
    [units.bytes]
  );

  return (
    <div {...brushScopeProps} className="contents">
      <Card
        accent="mem"
        labelledBy={titleId}
        className="flex flex-col gap-4 p-4"
      >
        <h2 id={titleId} className="sr-only">
          Memory composition
        </h2>
        <div className="flex flex-wrap items-end justify-between gap-x-7 gap-y-3">
          <StatStrip
            hero={{
              label: "Used",
              value:
                used === null
                  ? MISSING
                  : bytesParts(used, { units: units.bytes, unit: units.bytes })
                      .value,
              unit: totalGb === null ? undefined : `/ ${totalGb} GB`,
            }}
            items={[
              {
                label: "Pressure",
                value: formatPercent(at("mem.pressure")),
                secondary: state ?? undefined,
              },
              { label: "Swap", value: fmt(at("mem.swap_used")) },
              { label: "Compressed", value: fmt(at("mem.compressed")) },
            ]}
          />
          <RangeTotalsStrip
            windowMs={windowMs}
            totals={totals}
            testId="memory-range-totals"
          />
        </div>
        <StackBar
          accent="mem"
          showLegend
          legendColumns={2}
          segments={[
            {
              key: "app",
              label: "App",
              value: fmt(at("mem.app")),
              fraction: frac(at("mem.app")),
              step: 1,
            },
            {
              key: "wired",
              label: "Wired",
              value: fmt(at("mem.wired")),
              fraction: frac(at("mem.wired")),
              step: 2,
            },
            {
              key: "compressed",
              label: "Compressed",
              value: fmt(at("mem.compressed")),
              fraction: frac(at("mem.compressed")),
              step: "hatch",
            },
            {
              key: "cached",
              label: "Cached files",
              value: fmt(at("mem.cached")),
              fraction: frac(at("mem.cached")),
              step: 4,
            },
            {
              key: "free",
              label: "Free",
              value: fmt(at("mem.free")),
              fraction: frac(at("mem.free")),
              step: "track",
            },
          ]}
        />
      </Card>
    </div>
  );
}
