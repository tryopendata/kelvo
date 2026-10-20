import { ratio } from "@core/chart-math";
import { bytesParts, formatBytes, formatPercent, MISSING } from "@core/format";
import type { MetricStat } from "@core/generated/bindings";
import { useMemo } from "react";
import { brushScopeProps } from "~/components/brush-overlay";
import { RangeTotalsStrip } from "~/components/range-totals-strip";
import { SectionCard } from "~/components/section-card";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { useUnits } from "~/hooks/use-units";
import { StackBar } from "~/widgets/stack-bar";
import { StatStrip, type StatStripItem } from "~/widgets/stat-strip";
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
  "mem.psi{stall=some}",
  "mem.psi{stall=full}",
];

type Segment = Parameters<typeof StackBar>[0]["segments"][number];

/**
 * The bar's segments in the layout. A Mac splits used memory into app, wired
 * and compressed; a Linux agent (D-115) reports used, cached and free, which
 * add up to MemTotal.
 */
function segmentsFor(
  has: (metric: string) => boolean,
  at: (k: string) => number | null,
  fmt: (x: number | null) => string,
  frac: (x: number | null) => number | null
): Segment[] {
  const seg = (key: string, label: string, step: Segment["step"]): Segment => ({
    key,
    label,
    value: fmt(at(`mem.${key}`)),
    fraction: frac(at(`mem.${key}`)),
    step,
  });
  const used: Segment[] = has("mem.app")
    ? [seg("app", "App", 1), seg("wired", "Wired", 2)]
    : [seg("used", "Used", 1)];
  if (has("mem.compressed"))
    used.push(seg("compressed", "Compressed", "hatch"));
  return [
    ...used,
    seg("cached", "Cached files", 4),
    seg("free", "Free", "track"),
  ];
}

/**
 * What memory holds right now: used of total, pressure with its state word,
 * swap and compressed, then the composition bar (the Overview Memory card and
 * the popover bar, at page scale). Beside the live strip, the peaks over the
 * chart window or the brushed range (D-099).
 */
export function CompositionCard({
  totalLabel,
  totalBytes,
  windowMs,
}: {
  /** "24 GB" on a Mac, MemTotal as measured elsewhere. */
  totalLabel: string | null;
  /** `HostInfo.mem_total_bytes`: the bar's denominator, as in the popover (D-092). */
  totalBytes: number | null;
  windowMs: number;
}) {
  const units = useUnits();
  const v = useHeld(HELD);
  const at = (k: string) => v[k] ?? null;
  const used = at("mem.used");
  // Shares of installed memory; a missing part is its own empty segment.
  const frac = (x: number | null) => ratio(x, totalBytes);
  const fmt = (x: number | null) => formatBytes(x, { units: units.bytes });
  const state = pressureState(at("mem.pressure_level"));
  const layout = useLayout();
  const has = (metric: string) => layout?.byMetric.has(metric) ?? false;
  const items: StatStripItem[] = [];
  if (has("mem.psi") && !has("mem.pressure")) {
    // Linux pressure stall information: time some, and all, tasks waited.
    items.push({
      label: "Pressure stall",
      value: formatPercent(at("mem.psi{stall=some}"), { decimals: 1 }),
      secondary: `full ${formatPercent(at("mem.psi{stall=full}"), { decimals: 1 })}`,
    });
  } else {
    items.push({
      label: "Pressure",
      value: formatPercent(at("mem.pressure")),
      secondary: state ?? undefined,
    });
  }
  items.push({ label: "Swap", value: fmt(at("mem.swap_used")) });
  if (has("mem.compressed") || !layout) {
    items.push({ label: "Compressed", value: fmt(at("mem.compressed")) });
  }
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
      <SectionCard
        accent="mem"
        variant="default"
        title="Memory composition"
        hiddenTitle
        className="gap-4"
      >
        <div className="flex flex-wrap items-end justify-between gap-x-7 gap-y-3">
          <StatStrip
            hero={{
              label: "Used",
              value:
                used === null
                  ? MISSING
                  : bytesParts(used, { units: units.bytes, unit: units.bytes })
                      .value,
              unit: totalLabel === null ? undefined : `/ ${totalLabel}`,
            }}
            items={items}
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
          segments={segmentsFor(has, at, fmt, frac)}
        />
      </SectionCard>
    </div>
  );
}
