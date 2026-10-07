import { stackSum } from "@core/chart-math";
import { cpuPowerNote } from "@core/cpu-power-source";
import { eventLabel } from "@core/events";
import { formatWatts } from "@core/format";
import { windowWords } from "@core/live-window";
import { useMemo } from "react";
import { SectionCard } from "~/components/section-card";
import { useEvents } from "~/hooks/use-events";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { useHeld, useLayout } from "~/hooks/use-ring";
import { useWindowSeries } from "~/hooks/use-window-series";
import { Legend } from "~/widgets/legend";
import { type PowerComponent, PowerStack } from "~/widgets/power-stack";

const COMPONENTS: {
  key: PowerComponent;
  metric: string;
  label: string;
  step: 1 | 2 | 3 | "hatch";
}[] = [
  { key: "cpu", metric: "power.cpu", label: "CPU", step: 1 },
  { key: "gpu", metric: "power.gpu", label: "GPU", step: 2 },
  { key: "ane", metric: "power.ane", label: "ANE", step: "hatch" },
  { key: "dram", metric: "power.dram", label: "DRAM", step: 3 },
];

/**
 * Stacked power by component over the chart window (D-091) from the ring.
 * A component is stacked when its series is in the layout and
 * measured at least once in the window; the stack breaks wherever a stacked
 * component has no value. On macOS 27 the PMP-derived components (CPU, ANE, DRAM, package) are
 * gaps most of the time (D-043), so the stack is mostly broken there rather
 * than showing GPU power as if it were the total.
 */
export function PowerStackCard({ windowMs }: { windowMs: number }) {
  const layout = useLayout();
  const gaps = useGapBands("power");
  const present = COMPONENTS.filter((c) => layout?.byMetric.has(c.metric));
  const now = useHeld([
    ...COMPONENTS.map((c) => c.metric),
    "power.package",
    "power.cpu_source",
  ]);
  const cpuNote = cpuPowerNote(now["power.cpu_source"] ?? null);
  const series = useWindowSeries(
    present.map((c) => c.metric),
    windowMs
  );
  const drawn = present.filter((c) =>
    (series.values[c.metric] ?? []).some((v) => v !== null)
  );
  const totals = stackTotals(drawn.map((c) => series.values[c.metric] ?? []));
  const yMax = useNiceCeiling(totals, series.tEndMs, 5, windowMs);
  // Package and ANE spikes (D-083) as markers; the chart drops
  // the ones that have scrolled out of its window.
  const events = useEvents(windowMs, null);
  const annotations = useMemo(
    () =>
      events
        .filter((e) => e.detail.kind === "power_spike")
        .map((e) => ({ tsMs: e.start_ms, label: eventLabel(e) })),
    [events]
  );

  return (
    <SectionCard
      accent="power"
      origin="tr"
      title={`Power by component, last ${windowWords(windowMs)}`}
      aside={
        <span className="data-mono text-[13px]">
          {formatWatts(now["power.package"])}{" "}
          <span className="text-[11px] text-muted-foreground">package</span>
        </span>
      }
    >
      <Legend
        accent="power"
        items={COMPONENTS.map((c) => ({
          label: c.label,
          value: formatWatts(now[c.metric]),
          step: c.step,
        }))}
      />
      <PowerStack
        gaps={gaps}
        series={drawn.map((c) => ({
          key: c.key,
          values: series.values[c.metric] ?? [],
        }))}
        intervalMs={series.intervalMs}
        tEndMs={series.tEndMs}
        yMax={yMax}
        annotations={annotations}
      />
      {cpuNote && (
        <p className="m-0 text-[11px] text-muted-foreground">{cpuNote}</p>
      )}
    </SectionCard>
  );
}

/** Per-slot sum of the stacked series, `null` where any one is missing. */
export function stackTotals(
  series: readonly (readonly (number | null)[])[]
): (number | null)[] {
  const n = Math.max(0, ...series.map((s) => s.length));
  const out: (number | null)[] = [];
  for (let i = 0; i < n; i++) {
    out.push(series.length === 0 ? null : stackSum(series.map((s) => s[i])));
  }
  return out;
}
