import { scale, UNIT_LADDERS } from "./bytes";
import { fixed } from "./number";

/**
 * A formatted figure taken apart so a ticker can count between two of them
 * (opendata's `RollingNumber` parse, extended with the byte and rate unit
 * ladders so "10 GB" can count down to "100 MB").
 */
export interface Figure {
  prefix: string;
  /** The suffix with its leading space, exactly as shown (" GB", "%"). */
  suffix: string;
  decimals: number;
  grouped: boolean;
  /** Numeric value in the ladder's base unit (bytes), or as shown. */
  base: number;
  /** Which `UNIT_LADDERS` entry the suffix belongs to, and the step on it. */
  ladder: number;
  rung: number;
}

const FIGURE = /^([^\d−-]*)([−-]?[\d,]*\.?\d+)(.*)$/;

export function parseFigure(text: string): Figure | null {
  const m = FIGURE.exec(text);
  if (!m) return null;
  const [, prefix = "", raw = "", suffix = ""] = m;
  const n = Number(raw.replace("−", "-").replace(/,/g, ""));
  if (!Number.isFinite(n)) return null;
  const dot = raw.indexOf(".");
  const unit = suffix.trim();
  let ladder = -1;
  let rung = 0;
  for (const [i, l] of UNIT_LADDERS.entries()) {
    const at = l.units.indexOf(unit);
    if (at >= 0 && suffix.startsWith(" ")) {
      ladder = i;
      rung = at;
      break;
    }
  }
  const step = ladder >= 0 ? (UNIT_LADDERS[ladder]?.step ?? 1) : 1;
  return {
    prefix,
    suffix,
    decimals: dot === -1 ? 0 : raw.length - dot - 1,
    grouped: raw.includes(","),
    base: n * step ** rung,
    ladder,
    rung,
  };
}

/**
 * `target`'s text at an in-between `base` value. On a unit ladder the unit
 * moves with the value, the way the formatter would pick it; otherwise the
 * target's precision and grouping are kept.
 */
export function figureAt(target: Figure, base: number): string {
  const l = UNIT_LADDERS[target.ladder];
  if (l) {
    const q = scale(base, l.units, l.step, undefined, undefined);
    return `${target.prefix}${q.value} ${q.unit}`;
  }
  const s = target.grouped
    ? base.toLocaleString("en-US", {
        minimumFractionDigits: target.decimals,
        maximumFractionDigits: target.decimals,
      })
    : fixed(base, target.decimals);
  return `${target.prefix}${s}${target.suffix}`;
}

/** Below this relative change a value is replaced in place. */
export const TICK_MIN_CHANGE = 0.1;

/**
 * How to count from `from` to `to`, or null to replace the text in place.
 * Counts only between figures of the same kind (prefix, and unit ladder or
 * suffix) that moved by at least 10% or changed unit, and moved by at least
 * three display steps, so a 5% to 6% blip doesn't animate. Ladder figures
 * count in log space, so 10 GB to 100 MB spends as long in the megabytes as
 * in the gigabytes.
 */
export function tickPath(
  from: Figure | null,
  to: Figure | null
): { from: number; to: number; log: boolean } | null {
  if (!from || !to || from.prefix !== to.prefix) return null;
  if (from.ladder !== to.ladder) return null;
  if (to.ladder < 0 && from.suffix !== to.suffix) return null;
  const a = from.base;
  const b = to.base;
  if (a === b) return null;
  const rel = Math.abs(b - a) / Math.max(Math.abs(a), Math.abs(b));
  if (rel < TICK_MIN_CHANGE && from.rung === to.rung) return null;
  if (to.ladder >= 0) {
    // Log space needs both ends positive; 0 B/s to 40 MB/s just lands.
    if (a <= 0 || b <= 0) return null;
    return { from: Math.log(a), to: Math.log(b), log: true };
  }
  if (Math.abs(b - a) < 3 * 10 ** -to.decimals) return null;
  return { from: a, to: b, log: false };
}
