import { gridIntervalMs } from "@core/live-state";
import { windowWords } from "@core/live-window";
import { sk } from "@core/series-key";
import { heatmapBucketMs } from "@core/series-stats";
import { useDeferredValue } from "react";
import { ChartHistoryStatus } from "~/components/chart-history-status";
import { SectionCard } from "~/components/section-card";
import { useGapBands } from "~/hooks/use-gap-bands";
import { useModuleBuckets } from "~/hooks/use-module-buckets";
import { useHeld } from "~/hooks/use-ring";
import { useModuleRange } from "~/stores/chart-range";
import { useHost } from "~/stores/host-store";
import { CoreHeatmap } from "~/widgets/core-heatmap";
import { HeatScaleLegend } from "~/widgets/heat-matrix";
import type { ClusterView } from "../_lib/clusters";

/**
 * Per-core load over the chart window (D-091) in about 60 columns from the
 * ring: 15 s columns at 15m, wider when sampling is slower. A
 * column with no samples (before the app started, asleep, paused) is hatched
 * by the widget, never drawn as 0%.
 */
export function CoreLoadCard({
  clusters,
  windowMs: windowProp,
}: {
  clusters: readonly ClusterView[];
  windowMs: number;
}) {
  // A new window changes every column's width, so every cell (cores × ~60)
  // remounts. Deferred, that render runs as a transition React can split
  // across frames instead of one long task on the window control's click.
  // A store update (tick or backfill chunk) during that render makes React
  // redo it synchronously; if the perf gate flakes here, keep cells mounted
  // across a window change instead (the Cell keys in core-heatmap.tsx).
  const windowMs = useDeferredValue(windowProp);
  const end = useModuleRange()?.endMs ?? null;
  const desiredBucketMs = heatmapBucketMs(
    windowMs,
    useHost((s) => gridIntervalMs(s.status))
  );
  const span =
    end === null ? `last ${windowWords(windowMs)}` : "selected dates";
  const cores = clusters.flatMap((c) =>
    c.cores.map((id) => ({
      id,
      letter: c.letter,
      key: sk("cpu.load", { core: id }),
    }))
  );
  const keys = cores.map((c) => c.key);
  const now = useHeld(keys);
  const gaps = useGapBands("cpu");
  const { values, endMs, bucketMs, loading, error } = useModuleBuckets(
    keys,
    desiredBucketMs,
    Math.ceil(windowMs / desiredBucketMs),
    windowMs
  );

  return (
    <SectionCard
      accent="cpu"
      origin="bl"
      title={`Per-core load, ${span}`}
      aside={<HeatScaleLegend />}
      headerAlign="center"
      compactHeader
      className="col-span-2 gap-3"
    >
      <ChartHistoryStatus loading={loading} error={error} />
      <CoreHeatmap
        live={end === null}
        gaps={gaps}
        cores={cores.map((c) => ({
          id: c.id,
          cluster: c.letter,
          now: now[c.key] ?? null,
          buckets: values[c.key] ?? [],
        }))}
        bucketMs={bucketMs}
        windowMs={windowMs}
        endMs={endMs}
        ariaLabel={`Per-core load, ${span}, ${bucketMs / 1000} second columns`}
      />
    </SectionCard>
  );
}
