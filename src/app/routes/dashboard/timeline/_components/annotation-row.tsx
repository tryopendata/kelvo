import { formatClock } from "@core/format";
import { cn } from "~/lib/utils";
import { layoutMarkers, type Marker } from "../_lib/gaps";

export interface AnnotationRowProps {
  markers: readonly Marker[];
  fromMs: number;
  toMs: number;
  widthPx: number;
  /** An event pill was clicked: move the crosshair to `tMs`. */
  onSelect?: (tMs: number) => void;
}

/**
 * Mono 10 px is about 6 px per character, plus the 7 px dot and its gap.
 * An event pill is Inter 11 px (about 5.8 px per character) after a mono
 * "14:02 ", plus its padding, the 8 px dot and the gaps.
 */
const measure = (m: Marker) =>
  m.kind === "event"
    ? Math.ceil(6 * 6.6 + m.label.length * 5.8 + 32)
    : m.label.length * 6 + 16;
const MARKER_ROW_PX = 13;
const PILL_ROW_PX = 26;

/**
 * Detector and alert events as amber pills, and Sleep and Wake markers (a
 * hollow dot for Sleep, a filled one for Wake), above the lanes.
 * Pills never overlap: a crowded one moves to a second row or merges into
 * "+N" on its nearest neighbour. Events and Sleep/Wake are laid out apart,
 * the markers on their own rows under the pills, so a wide event pill never
 * swallows a Sleep or Wake marker into its "+N". Clicking an event pill moves the crosshair to it.
 */
export function AnnotationRow({
  markers,
  fromMs,
  toMs,
  widthPx,
  onSelect,
}: AnnotationRowProps) {
  const lay = (kind: (m: Marker) => boolean) =>
    widthPx > 0
      ? layoutMarkers(markers.filter(kind), fromMs, toMs, widthPx, measure)
      : [];
  const events = lay((m) => m.kind === "event");
  const marks = lay((m) => m.kind !== "event");
  const rowsOf = (ps: readonly { row: number }[]) =>
    ps.length === 0 ? 0 : Math.max(...ps.map((p) => p.row + 1));
  const pillsPx = rowsOf(events) * PILL_ROW_PX;
  const placed = [...events, ...marks];
  return (
    <ul
      aria-label="Events, sleep and wake"
      className="relative"
      style={{
        height: Math.max(26, pillsPx + rowsOf(marks) * MARKER_ROW_PX + 4),
      }}
    >
      {placed.map((p) =>
        p.kind === "event" ? (
          <li
            key={`event-${p.tMs}-${p.label}`}
            className="absolute"
            // Anchored by its right edge, so the dot lands on the time
            // whatever the text really measures; layout used the estimate.
            style={{
              right: widthPx - p.leftPx - measure(p),
              top: 1 + p.row * PILL_ROW_PX,
            }}
          >
            <button
              type="button"
              data-event-ts={p.tMs}
              className="flex h-[22px] items-center gap-2 whitespace-nowrap rounded-full border border-warning/40 bg-warning/10 pr-1.5 pl-2.5 text-[11px] text-foreground outline-none hover:bg-warning/20 focus-visible:ring-2 focus-visible:ring-ring"
              onClick={() => onSelect?.(p.tMs)}
            >
              <span>
                <span className="data-mono text-power-ink">
                  {formatClock(p.tMs)}
                </span>{" "}
                {p.label}
                {p.more > 0 && (
                  <span className="text-muted-foreground"> +{p.more}</span>
                )}
              </span>
              <span aria-hidden className="size-2 rounded-full bg-warning" />
            </button>
          </li>
        ) : (
          <li
            key={`${p.kind}-${p.tMs}`}
            className="absolute flex items-center gap-1.5"
            style={{ left: p.leftPx, top: pillsPx + 4 + p.row * MARKER_ROW_PX }}
          >
            <span
              aria-hidden
              className={cn(
                "size-[7px] rounded-full border border-muted-foreground",
                p.kind === "wake" && "bg-muted-foreground"
              )}
            />
            <span className="data-mono whitespace-nowrap text-[10px] text-muted-foreground">
              {p.label}
              {p.more > 0 && ` +${p.more}`}
            </span>
          </li>
        )
      )}
    </ul>
  );
}
