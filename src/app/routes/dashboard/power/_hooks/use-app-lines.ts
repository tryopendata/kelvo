import { windowRange } from "@core/brush";
import { resampleBuckets, type SlotGrid } from "@core/chart-math";
import { useMemo } from "react";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import { useOpenEdge } from "~/hooks/use-range-scope";
import { useUsageSeries } from "~/hooks/use-usage-series";

/** Apps drawn as lines under the system draw. */
export const APP_LINES = 5;

export interface AppLine {
  name: string;
  /** Average watts over the window's sampled time. */
  avg: number | null;
  /** One per chart slot, from the app's 10 s buckets. */
  values: (number | null)[];
  /** The largest of `values`, for the chart's ceiling. */
  peak: number | null;
}

export type AppLines =
  /** Per-app energy is kept only on the Mac Kelvo runs on (D-093). */
  | { kind: "remote" }
  | { kind: "loading" }
  | { kind: "error" }
  /** Nothing measured in the window yet (just launched). */
  | { kind: "empty" }
  | { kind: "ok"; apps: AppLine[] };

/**
 * The top apps by energy over the chart window (D-093), as lines on the
 * power chart's slot grid. The window ends where the open 10 s bucket
 * starts, so the read moves every 10 s; the previous answer stands in
 * meanwhile. The answer is laid on the grid by time, so a held answer
 * still lines up.
 */
export function useAppLines(windowMs: number, grid: SlotGrid): AppLines {
  const local = useIsLocalHost();
  const edge = useOpenEdge();
  const range = useMemo(
    () => (edge === null ? null : windowRange(edge, windowMs)),
    [edge, windowMs]
  );
  const q = useUsageSeries(range, "energy", APP_LINES, { keepPrevious: true });
  const data = q.data;
  const { tEndMs, intervalMs, length } = grid;
  const apps = useMemo(
    () =>
      data?.apps.map((app) => ({
        name: app.name,
        avg: app.avg,
        values: resampleBuckets(
          {
            fromMs: data.from_ms,
            bucketMs: data.bucket_ms,
            values: app.values,
          },
          { tEndMs, intervalMs, length }
        ),
        peak: app.values.reduce<number | null>(
          (m, v) => (v !== null && (m === null || v > m) ? v : m),
          null
        ),
      })) ?? [],
    [data, tEndMs, intervalMs, length]
  );

  let lines: AppLines;
  if (!local) lines = { kind: "remote" };
  else if (q.isError) lines = { kind: "error" };
  else if (!data) lines = { kind: "loading" };
  else if (apps.length === 0) lines = { kind: "empty" };
  else lines = { kind: "ok", apps };
  return lines;
}
