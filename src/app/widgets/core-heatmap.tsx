import { heatmapAlpha } from "@core/chart-math";
import { memo, type ReactElement, useRef } from "react";
import { cn } from "~/lib/utils";
import { GapBands, type GapSpan } from "./gap-band";
import { type Accent, accentVars } from "./lib/accent";
import { windowTicks } from "./lib/chart-labels";

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

const HATCH =
  "repeating-linear-gradient(135deg, var(--color-grid) 0 1px, transparent 1px 4px)";

const Cell = memo(function Cell({ v }: { v: number | null }) {
  const a = heatmapAlpha(v, 100);
  return (
    <span
      data-gap={a === null || undefined}
      className="h-2.5 rounded-hair"
      style={{
        background:
          a === null
            ? HATCH
            : `color-mix(in srgb, var(--a) ${(a * 100).toFixed(0)}%, transparent)`,
      }}
    />
  );
});

interface ClosedCellsProps {
  values: readonly (number | null)[];
  /** Key of the first cell; the rest count up from it. */
  firstKey: number;
}

const sameCells = (a: ClosedCellsProps, b: ClosedCellsProps) =>
  a.firstKey === b.firstKey &&
  a.values.length === b.values.length &&
  a.values.every((v, i) => v === b.values[i]);

/**
 * The closed columns of one row. Only the newest column changes between
 * closes, so this re-renders when a column closes (or the data changes),
 * not on every tick: a row's 1 Hz cost is one cell, not the whole window.
 * Keyed by bucket, a close renders the one new cell; the others reuse last
 * render's element, so React skips them without building an element or
 * comparing props (on a 16-core CPU page that was ~1,000 elements every 10 s).
 */
const ClosedCells = memo(function ClosedCells({
  values,
  firstKey,
}: ClosedCellsProps) {
  const last = useRef(
    new Map<number, { v: number | null; el: ReactElement }>()
  );
  const next = new Map<number, { v: number | null; el: ReactElement }>();
  const cells = values.map((v, i) => {
    // The bucket number (or the position, without `endMs`), not the index.
    const key = firstKey + i;
    const prev = last.current.get(key);
    const el = prev && prev.v === v ? prev.el : <Cell key={key} v={v} />;
    next.set(key, { v, el });
    return el;
  });
  last.current = next;
  return cells;
}, sameCells);

/**
 * Per-core load over time: one row per core, one cell per
 * bucket, alpha `0.06 + v · 0.89` of the accent. A bucket with no samples
 * (before app start, inside a gap) is hatched, never the faintest tint. An
 * 8 px gap separates the clusters.
 */
export function CoreHeatmap({
  cores,
  bucketMs,
  windowMs,
  endMs = null,
  accent = "cpu",
  ariaLabel = "Per-core load",
  gaps,
}: CoreHeatmapProps) {
  const cols = Math.max(1, Math.round(windowMs / bucketMs));
  // Bucket number of the first column, or 0 for positional keys.
  const firstKey = endMs === null ? 0 : Math.floor(endMs / bucketMs) - cols + 1;
  const ticks = windowTicks(windowMs, 3);
  // Rows are placed explicitly so the gap band can span the cell column of
  // every core row without the auto-placement flowing around it.
  const row = (k: number) => ({ gridRow: k + 1 });
  return (
    <div
      role="img"
      aria-label={ariaLabel}
      className="grid grid-cols-[28px_minmax(0,1fr)_40px] items-center gap-x-2.5 gap-y-0.5"
      style={accentVars(accent)}
    >
      {cores.map((core, k) => {
        const first = k > 0 && core.cluster !== cores[k - 1]?.cluster;
        // Right-align: the newest bucket is always the last column.
        const pad = Math.max(0, cols - core.buckets.length);
        const cells = [
          ...Array<number | null>(pad).fill(null),
          ...core.buckets.slice(-cols),
        ];
        return (
          <div key={core.id} className={cn("contents", first && "[&>*]:mt-2")}>
            <span
              className="data-mono col-start-1 text-[10px] text-muted-foreground"
              style={row(k)}
            >
              {core.id}
            </span>
            <div
              className="col-start-2 grid gap-px"
              style={{
                ...row(k),
                gridTemplateColumns: `repeat(${cols}, minmax(0, 1fr))`,
              }}
            >
              <ClosedCells values={cells.slice(0, -1)} firstKey={firstKey} />
              <Cell v={cells[cells.length - 1] ?? null} />
            </div>
            <span
              className="data-mono col-start-3 text-right text-[11px]"
              style={row(k)}
            >
              {core.now == null ? "–" : `${Math.round(core.now)}%`}
            </span>
          </div>
        );
      })}
      {gaps && gaps.length > 0 && endMs !== null && cores.length > 0 && (
        <div
          className="pointer-events-none relative col-start-2 self-stretch"
          style={{ gridRow: `1 / ${cores.length + 1}` }}
        >
          <GapBands
            gaps={gaps}
            rangeFromMs={firstKey * bucketMs}
            rangeToMs={(firstKey + cols) * bucketMs}
          />
        </div>
      )}
      <span className="col-start-1" style={row(cores.length)} />
      <div
        className="col-start-2 flex justify-between pt-1.5"
        style={row(cores.length)}
        aria-hidden
      >
        {ticks.map((t, i) => (
          <span
            key={t}
            className={cn(
              "data-mono text-[10px]",
              i === ticks.length - 1 ? "text-muted-foreground" : "text-fg-faint"
            )}
          >
            {t}
          </span>
        ))}
      </div>
      <span className="col-start-3" style={row(cores.length)} />
    </div>
  );
}

/** The 0% to 100% swatch legend that sits in the heatmap card header. */
export function HeatScaleLegend({ accent = "cpu" }: { accent?: Accent }) {
  return (
    <div
      aria-hidden
      className="flex items-center gap-1"
      style={accentVars(accent)}
    >
      <span className="data-mono mr-1 text-[10px] text-fg-faint">0%</span>
      {[8, 35, 65, 95].map((p) => (
        <span
          key={p}
          className="h-2 w-3.5 rounded-mark"
          style={{
            background: `color-mix(in srgb, var(--a) ${p}%, transparent)`,
          }}
        />
      ))}
      <span className="data-mono ml-1 text-[10px] text-fg-faint">100%</span>
    </div>
  );
}
