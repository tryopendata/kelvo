import { formatClockSeconds } from "@core/format";
import { type RefObject, useLayoutEffect, useRef } from "react";
import {
  ChartTooltipShell,
  SeriesSwatch,
} from "~/components/charts/chart-tooltip";
import type { GapSpan } from "~/widgets/gap-band";
import { type Accent, accentVars, rampColor } from "~/widgets/lib/accent";
import { FIELD_LABEL } from "~/widgets/lib/classes";

/** Bar slot under a pointer at `clientX`, or null off the bars. */
export function slotAt(
  clientX: number,
  box: { left: number; width: number },
  count: number
): number | null {
  if (box.width <= 0) return null;
  const f = (clientX - box.left) / box.width;
  if (f < 0 || f >= 1) return null;
  return Math.min(count - 1, Math.floor(f * count));
}

/** Label of the first gap that overlaps `[fromMs, toMs)`, if any. */
function gapAt(
  gaps: readonly GapSpan[] | undefined,
  fromMs: number,
  toMs: number
): string | null {
  return gaps?.find((g) => g.fromMs < toMs && g.toMs > fromMs)?.label ?? null;
}

export interface MirrorHoverProps {
  /** The bars' box: pointer x is measured against it. */
  plotRef: RefObject<HTMLElement | null>;
  /** Width of the label column left of the bars, px. */
  inset: number;
  height: number;
  count: number;
  firstMs: number;
  bucketMs: number;
  up: readonly (number | null)[];
  down: readonly (number | null)[];
  upLabel: string;
  downLabel: string;
  gaps?: readonly GapSpan[];
  format: (v: number) => string;
  accent: Accent;
  /** Draw a column over the hovered bar (off when the brush draws its own). */
  column: boolean;
}

/**
 * Hover readout for a mirrored chart: the hovered bar's time span and both
 * sides' averages, styled after the Timeline crosshair tooltip.
 * A bar with no samples says so, with the gap's reason when one covers it,
 * rather than reading as 0.
 *
 * Like `BrushOverlay`, it listens on its parent and writes the DOM through
 * refs, so a pointer passing over the chart re-renders nothing. Each tick's
 * render repaints it with the new values.
 */
/** Fill and place the tooltip and column for bar `i`, or hide them. */
function paint(
  tip: HTMLElement,
  col: HTMLElement,
  p: MirrorHoverProps,
  i: number | null
) {
  if (i === null || i >= p.count) {
    tip.hidden = true;
    col.hidden = true;
    return;
  }
  const fromMs = p.firstMs + i * p.bucketMs;
  const toMs = fromMs + p.bucketMs;
  const u = p.up[i] ?? null;
  const d = p.down[i] ?? null;
  const empty = u === null && d === null;
  const field = (name: string) =>
    tip.querySelector<HTMLElement>(`[data-field="${name}"]`);
  const set = (name: string, text: string) => {
    const el = field(name);
    if (el && el.textContent !== text) el.textContent = text;
  };
  set(
    "span",
    p.bucketMs > 1000
      ? `${formatClockSeconds(fromMs)}–${formatClockSeconds(toMs)}`
      : formatClockSeconds(fromMs)
  );
  set("res", i === p.count - 1 ? "so far" : `${p.bucketMs / 1000} s avg`);
  set("up", u === null ? "—" : p.format(u));
  set("down", d === null ? "—" : p.format(d));
  set("note", gapAt(p.gaps, fromMs, toMs) ?? "No samples");
  const note = field("note");
  const rows = field("rows");
  if (note) note.hidden = !empty;
  if (rows) rows.hidden = empty;
  // Beside the bar, on whichever side has more room.
  const plot = `(100% - ${p.inset}px)`;
  if (i > p.count * 0.6) {
    tip.style.left = "";
    tip.style.right = `calc(${plot} * ${1 - i / p.count} + 8px)`;
  } else {
    tip.style.right = "";
    tip.style.left = `calc(${p.inset}px + ${plot} * ${(i + 1) / p.count} + 8px)`;
  }
  tip.hidden = false;
  col.style.left = `calc(${p.inset}px + ${plot} * ${i / p.count})`;
  col.style.width = `calc(${plot} / ${p.count})`;
  col.hidden = !p.column;
}

/**
 * Hover readout for a mirrored chart: the hovered bar's time span and both
 * sides' averages, styled after the Timeline crosshair tooltip.
 * A bar with no samples says so, with the gap's reason when one covers it,
 * rather than reading as 0.
 *
 * Like `BrushOverlay`, it listens on its parent and writes the DOM through
 * refs, so a pointer passing over the chart re-renders nothing. Each tick's
 * render repaints it with the new values.
 */
export function MirrorHover(props: MirrorHoverProps) {
  const { height, upLabel, downLabel, accent } = props;
  const latest = useRef(props);
  const slot = useRef<number | null>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const colRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    latest.current = props;
    if (tipRef.current && colRef.current)
      paint(tipRef.current, colRef.current, props, slot.current);
  });

  useLayoutEffect(() => {
    const tip = tipRef.current;
    const col = colRef.current;
    const host = tip?.parentElement;
    if (!tip || !col || !host) return;
    const move = (e: globalThis.PointerEvent) => {
      const p = latest.current;
      const box = p.plotRef.current?.getBoundingClientRect();
      const next = box ? slotAt(e.clientX, box, p.count) : null;
      if (next === slot.current) return;
      slot.current = next;
      paint(tip, col, p, next);
    };
    const leave = () => {
      slot.current = null;
      paint(tip, col, latest.current, null);
    };
    host.addEventListener("pointermove", move);
    host.addEventListener("pointerleave", leave);
    return () => {
      host.removeEventListener("pointermove", move);
      host.removeEventListener("pointerleave", leave);
    };
  }, []);

  return (
    <>
      <div
        ref={colRef}
        hidden
        aria-hidden
        className="pointer-events-none absolute top-0 bg-foreground/4"
        style={{ height }}
      />
      <ChartTooltipShell
        ref={tipRef}
        hidden
        className="top-0 w-[220px]"
        style={accentVars(accent)}
      >
        <div className="flex items-baseline justify-between gap-2">
          <span data-field="span" className="figures text-[12px]" />
          <span data-field="res" className={FIELD_LABEL} />
        </div>
        <span
          data-field="note"
          className="font-normal text-[12px] text-muted-foreground"
        />
        <div data-field="rows" className="flex flex-col gap-[5px]">
          {(
            [
              [upLabel, 2, "up"],
              [downLabel, 1, "down"],
            ] as const
          ).map(([label, step, field]) => (
            <div key={label} className="flex items-center gap-2 text-[12px]">
              <SeriesSwatch color={rampColor(step)} />
              <span className="flex-1 font-normal text-fg-subtle">{label}</span>
              <span data-field={field} className="figures" />
            </div>
          ))}
        </div>
      </ChartTooltipShell>
    </>
  );
}
