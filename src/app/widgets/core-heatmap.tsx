import type { GapSpan } from "./gap-band";
import { HeatMatrix } from "./heat-matrix";
import type { Accent } from "./lib/accent";

export interface CoreHeatmapProps {
  /** One row per core, P cores first. `buckets` are oldest first; `null` is no samples. */
  cores: {
    id: string;
    cluster: "P" | "E";
    /** Current load in percent, or null when stale. */
    now: number | null;
    buckets: (number | null)[];
  }[];
  bucketMs: number;
  windowMs: number;
  live?: boolean;
  /**
   * Start of the newest bucket (ms epoch). Cells are then keyed by bucket,
   * so when a column closes and the window slides, existing cells keep
   * their identity and only the new one renders. Without it cells are
   * keyed by position.
   */
  endMs?: number | null;
  accent?: Accent;
  ariaLabel?: string;
  /**
   * Labelled gaps drawn as one hatched band over every core row.
   * Needs `endMs` to place them.
   */
  gaps?: readonly GapSpan[];
}

const LOAD_SCALE = { min: 0, max: 100 };
const percent = (v: number) => `${Math.round(v)}%`;

/**
 * Per-core load over time: `HeatMatrix` on a 0 to 100% scale, one row per
 * core, grouped by cluster. JSON props, so desktop boards can carry it.
 */
export function CoreHeatmap({
  cores,
  accent = "cpu",
  ariaLabel = "Per-core load",
  ...rest
}: CoreHeatmapProps) {
  return (
    <HeatMatrix
      {...rest}
      rows={cores.map((c) => ({
        id: c.id,
        label: c.id,
        group: c.cluster,
        now: c.now,
        buckets: c.buckets,
      }))}
      scale={LOAD_SCALE}
      formatNow={percent}
      accent={accent}
      ariaLabel={ariaLabel}
    />
  );
}
