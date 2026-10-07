import { cn } from "~/lib/utils";

export interface ChartAnnotationProps {
  /** When the annotated moment happened (ms epoch); the chart positions it. */
  tsMs: number;
  /** "ANE 1.4 W · Photos face analysis". */
  label: string;
  /**
   * `marker`: a 20 px vertical rule then the label (power chart).
   * `pill`: a bordered label on the card surface (battery chart).
   */
  variant?: "marker" | "pill";
}

/**
 * Labelled marker inside a chart. Render-only: the chart wraps it
 * in a box positioned at `tsMs` on its own x scale.
 */
export function ChartAnnotation({
  tsMs,
  label,
  variant = "marker",
}: ChartAnnotationProps) {
  return (
    <span
      data-annotation-ts={tsMs}
      className={cn(
        "inline-flex items-center gap-1.5 whitespace-nowrap text-[11px] text-fg-subtle",
        variant === "pill" &&
          "rounded-mark border border-border bg-card px-1.5 py-0.5"
      )}
    >
      {variant === "marker" && (
        <span aria-hidden className="h-5 w-px bg-border-strong" />
      )}
      {label}
    </span>
  );
}

/**
 * Horizontal position of `tsMs` as a fraction of a window ending at `tEndMs`,
 * or null when it falls outside.
 */
export function annotationFraction(
  tsMs: number,
  tEndMs: number,
  windowMs: number
): number | null {
  if (windowMs <= 0) return null;
  const f = 1 - (tEndMs - tsMs) / windowMs;
  return f < 0 || f > 1 ? null : f;
}
