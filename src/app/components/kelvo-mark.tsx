import { cn } from "~/lib/utils";

/** Fill opacity of each heat-grid cell, row by row; the newest (bottom-right) is solid. */
const CELLS = [
  [0.2, 0.35, 0.25, 0.5],
  [0.45, 0.2, 0.6, 0.3],
  [0.3, 0.7, 0.4, 0.8],
  [0.55, 0.35, 0.85, 1],
] as const;

/**
 * The Kelvo mark: a 4 x 4 slice of the history heatmap in the CPU accent.
 * Same geometry as `brand/kelvo-icon.svg`, with the viewBox cropped to the
 * cells so the grid's edge lines up with the text beside it. In dark mode the faint cells lift
 * 20% toward solid, as in `brand/kelvo-icon-dark.svg`, or they vanish on dark
 * surfaces. Decorative wherever the name sits next to it, so it is hidden from
 * assistive tech.
 */
export function KelvoMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="1 1 30 30"
      aria-hidden
      className={cn(
        "shrink-0 fill-cpu [--lift:0] dark:[--lift:0.2]",
        className
      )}
    >
      {CELLS.flatMap((row, j) =>
        row.map((o, i) => (
          <rect
            // biome-ignore lint/suspicious/noArrayIndexKey: fixed grid, position is the identity
            key={`${i}-${j}`}
            x={1 + i * 8}
            y={1 + j * 8}
            width="6"
            height="6"
            rx="1.5"
            style={{ fillOpacity: `calc(${o} + ${1 - o} * var(--lift))` }}
          />
        ))
      )}
    </svg>
  );
}
