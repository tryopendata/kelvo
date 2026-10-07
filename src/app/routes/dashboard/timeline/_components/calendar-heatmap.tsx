import { type KeyboardEvent, memo, useEffect, useRef, useState } from "react";
import { useThemeVersion, withAlpha } from "~/components/charts/theme";
import { useElementSize } from "~/hooks/use-element-size";
import { cn } from "~/lib/utils";
import {
  cellAlpha,
  type HeatmapRow,
  type HeatmapScale,
  isFutureCell,
  isPendingCell,
} from "../_lib/heatmap";

/**
 * 11 px cells 2 px apart with 2 px corners, and a 72 px label
 * column 8 px from the cells (78 px plus the 2 px column gap here).
 */
const CELL_H = 11;
const GAP = 2;
const CELLS_LEFT = 80;
/** Room around the cells for the current hour's 1.5 px outline. */
const PAD = 2;
const HOURS = Array.from({ length: 24 }, (_, h) => h);
const ROW_TEMPLATE = "grid grid-cols-[78px_repeat(24,minmax(0,1fr))] gap-x-0.5";

export interface CalendarHeatmapProps {
  rows: readonly HeatmapRow[];
  scale: HeatmapScale;
  /** The cell holding now, `[row, hour]`: outlined, and its row's label bright. */
  current: [number, number] | null;
  /**
   * The start of the current local hour. Later cells have not happened:
   * drawn as plain track (no hatch, which means "no samples"), disabled.
   */
  nowHourMs: number;
  /** No values yet: every cell is plain track, the grid is busy. */
  loading: boolean;
  /** The accessible name and tooltip of a cell. */
  cellName: (row: HeatmapRow, hour: number) => string;
  onSelect: (row: HeatmapRow, hour: number) => void;
  ariaLabel: string;
}

/**
 * Days by hours (design-system.md "CalendarHeatmap").
 * The cells are painted on one canvas, redrawn when the rows, the scale, the
 * width, the theme or the hour change: at most every 5 minutes, not per
 * frame. Over it sits a table with the grid role whose transparent cells
 * carry the names, the tooltips, the focus ring and the clicks, with one tab
 * stop and arrow keys.
 */
export const CalendarHeatmap = memo(function CalendarHeatmap({
  rows,
  scale,
  current,
  nowHourMs,
  loading,
  cellName,
  onSelect,
  ariaLabel,
}: CalendarHeatmapProps) {
  const selectable = (row: HeatmapRow, h: number) =>
    !isFutureCell(row, h, nowHourMs);
  const [bodyRef, width] = useElementSize("width");
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const theme = useThemeVersion();
  const [focus, setFocus] = useState<[number, number] | null>(null);
  const [fr, fh] = focus ?? current ?? [rows.length - 1, 0];

  const height = rows.length * (CELL_H + GAP) - GAP;
  const cellsW = Math.max(0, width - CELLS_LEFT);
  const curRow = current?.[0];
  const curHour = current?.[1];

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx || cellsW <= 0) return;
    void theme;
    const css = getComputedStyle(canvas);
    const fill = css.getPropertyValue(`--color-${scale.accent}`).trim();
    const hatch = css.getPropertyValue("--color-grid").trim();
    const ring = css.getPropertyValue("--color-foreground").trim();
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round((cellsW + 2 * PAD) * dpr);
    canvas.height = Math.round((height + 2 * PAD) * dpr);
    ctx.setTransform(dpr, 0, 0, dpr, PAD * dpr, PAD * dpr);
    ctx.clearRect(-PAD, -PAD, cellsW + 2 * PAD, height + 2 * PAD);
    const colW = (cellsW - 23 * GAP) / 24;
    const cell = (x: number, y: number, w: number, h: number, r: number) => {
      ctx.beginPath();
      if (ctx.roundRect) ctx.roundRect(x, y, w, h, r);
      else ctx.rect(x, y, w, h);
    };
    rows.forEach((row, r) => {
      const y = r * (CELL_H + GAP);
      for (let h = 0; h < 24; h++) {
        const x = h * (colW + GAP);
        const v = row.hours[h];
        cell(x, y, colW, CELL_H, 2);
        // Not happened yet, not read yet, or this hour's minutes not committed
        // yet: plain track. The hatch says "no samples", which none of them is.
        if (
          loading ||
          isFutureCell(row, h, nowHourMs) ||
          isPendingCell(row, h, nowHourMs)
        ) {
          ctx.fillStyle = hatch;
          ctx.fill();
          continue;
        }
        if (v != null) {
          ctx.fillStyle = withAlpha(fill, cellAlpha(v, scale));
          ctx.fill();
          continue;
        }
        // The hatch: 1 px lines every 4 px at 135 degrees.
        ctx.save();
        ctx.clip();
        ctx.strokeStyle = hatch;
        ctx.lineWidth = 1;
        ctx.beginPath();
        for (let k = -CELL_H; k < colW; k += 4) {
          ctx.moveTo(x + k, y + CELL_H);
          ctx.lineTo(x + k + CELL_H, y);
        }
        ctx.stroke();
        ctx.restore();
      }
    });
    if (curRow !== undefined && curHour !== undefined) {
      cell(
        curHour * (colW + GAP) - 0.75,
        curRow * (CELL_H + GAP) - 0.75,
        colW + 1.5,
        CELL_H + 1.5,
        2.75
      );
      ctx.strokeStyle = ring;
      ctx.lineWidth = 1.5;
      ctx.stroke();
    }
  }, [rows, scale, curRow, curHour, nowHourMs, loading, cellsW, height, theme]);

  const onCellKey = (e: KeyboardEvent, r: number, h: number) => {
    const moves: Record<string, [number, number]> = {
      ArrowUp: [r - 1, h],
      ArrowDown: [r + 1, h],
      ArrowLeft: [r, h - 1],
      ArrowRight: [r, h + 1],
      Home: [r, 0],
      End: [r, 23],
    };
    const to = moves[e.key];
    if (to) {
      e.preventDefault();
      const nr = Math.min(rows.length - 1, Math.max(0, to[0]));
      const nh = Math.min(23, Math.max(0, to[1]));
      setFocus([nr, nh]);
      bodyRef.current
        ?.querySelector<HTMLElement>(`[data-cell="${nr}-${nh}"]`)
        ?.focus();
      return;
    }
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      const row = rows[r];
      if (row && selectable(row, h)) onSelect(row, h);
    }
  };

  return (
    <div className="flex flex-col gap-0.5">
      <div aria-hidden className={ROW_TEMPLATE}>
        <span />
        {HOURS.map((h) => (
          <span
            key={h}
            className="data-mono text-[9px] text-fg-faint leading-[11px]"
          >
            {h % 3 === 0 ? String(h).padStart(2, "0") : ""}
          </span>
        ))}
      </div>
      <div ref={bodyRef} className="relative">
        <canvas
          ref={canvasRef}
          aria-hidden
          data-heatmap-canvas
          className="pointer-events-none absolute"
          style={{
            left: CELLS_LEFT - PAD,
            top: -PAD,
            width: cellsW + 2 * PAD,
            height: height + 2 * PAD,
          }}
        />
        <table
          // biome-ignore lint/a11y/noNoninteractiveElementToInteractiveRole: a data grid with arrow keys is the heatmap's pattern (design-system.md, Accessibility)
          role="grid"
          aria-label={ariaLabel}
          aria-busy={loading}
          className="relative flex w-full flex-col gap-0.5"
        >
          <tbody className="contents">
            {rows.map((row, r) => (
              <tr key={row.date} className={ROW_TEMPLATE}>
                <th
                  scope="row"
                  className={cn(
                    "data-mono whitespace-nowrap text-left font-normal text-[10px] leading-[11px]",
                    r === curRow ? "text-foreground" : "text-muted-foreground"
                  )}
                >
                  {row.label}
                </th>
                {HOURS.map((h) => {
                  const name = cellName(row, h);
                  const enabled = selectable(row, h);
                  return (
                    <td
                      key={h}
                      // biome-ignore lint/a11y/noNoninteractiveElementToInteractiveRole: in a grid the cells are the focusable, clickable widgets; a td under role="grid" is not exposed as a gridcell without it
                      role="gridcell"
                      data-cell={`${r}-${h}`}
                      tabIndex={r === fr && h === fh ? 0 : -1}
                      aria-label={name}
                      aria-disabled={enabled ? undefined : true}
                      title={name}
                      onClick={() => {
                        setFocus([r, h]);
                        if (enabled) onSelect(row, h);
                      }}
                      onKeyDown={(e) => onCellKey(e, r, h)}
                      onFocus={() => setFocus([r, h])}
                      className={cn(
                        "h-[11px] rounded-[2px] p-0 outline-none focus-visible:outline-2 focus-visible:outline-ring focus-visible:outline-offset-2",
                        enabled && "cursor-pointer"
                      )}
                    />
                  );
                })}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
});
