import { formatBytes } from "@core/format";
import type { SpaceNode, SpaceSubtree } from "@core/generated/bindings";
import {
  FRAME_HEADER,
  labelWidth,
  layoutSpace,
  type SpaceTile,
  spaceEntries,
  spaceOrder,
  tileLabel,
} from "@core/space-treemap";
import { ChevronRight } from "lucide-react";
import {
  type CSSProperties,
  memo,
  type RefObject,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useElementSize } from "~/hooks/use-element-size";
import { cn } from "~/lib/utils";
import { canOpen, itemsLabel, kindLabel, shareLabel } from "../_lib/space";

const HATCH =
  "repeating-linear-gradient(135deg,var(--color-grid) 0 1px,transparent 1px 7px)";
const ramp = (step: 2 | 3 | 4) =>
  `color-mix(in srgb, var(--color-disk) var(--ramp-${step}), transparent)`;

/**
 * The treemap of one folder (plan/disk-space-report.md "The treemap"). Tiles
 * are the folder's entries, area by allocated bytes; a large enough folder is
 * a frame holding its own largest entries, so two levels read at once. Color
 * carries structure only. Tiles are `aria-hidden`: the table below is the
 * keyboard and screen-reader path, and this box names the folder and its
 * largest entries.
 */
export const SpaceMap = memo(function SpaceMap({
  subtree,
  hovered,
  onHover,
  onOpen,
}: {
  subtree: SpaceSubtree;
  hovered: string | null;
  onHover: (key: string | null) => void;
  onOpen: (node: SpaceNode) => void;
}) {
  const [ref, width] = useElementSize("width", 0);
  const height = Math.round(Math.min(440, Math.max(240, width * 0.48)));
  const folderId = subtree.folder.id ?? 0;
  // Finish order per folder, so a running scan appends tiles in place.
  const orders = useRef(new Map<number, string[]>());
  const tiles = useMemo(() => {
    const entries = spaceEntries(
      subtree.children,
      subtree.unmeasured_bytes,
      orders.current.get(folderId)
    );
    orders.current.set(folderId, spaceOrder(entries));
    return layoutSpace(entries, width, height);
  }, [subtree, width, height, folderId]);
  const layer = useRef<HTMLDivElement>(null);
  useZoom(layer, folderId, tiles, width, height);
  const tip = useRef<HTMLDivElement>(null);
  // The tip follows the pointer, so it shows only while the pointer is here,
  // not when a hovered table row highlights a tile.
  const [inside, setInside] = useState(false);
  const hoveredTile = inside
    ? (tiles.find((t) => t.key === hovered) ?? null)
    : null;
  return (
    <div
      ref={ref}
      role="img"
      aria-label={mapLabel(subtree)}
      className="relative w-full select-none"
      style={{ height }}
      onPointerMove={(e) => placeTip(tip.current, e)}
      onPointerEnter={() => setInside(true)}
      onPointerLeave={() => {
        setInside(false);
        onHover(null);
      }}
    >
      <div className="absolute inset-0 overflow-hidden rounded-[3px]">
        <div ref={layer} className="absolute inset-0 origin-top-left">
          {tiles.map((t) => (
            <Tile
              key={t.key}
              tile={t}
              active={t.key === hovered}
              onHover={onHover}
              onOpen={onOpen}
            />
          ))}
        </div>
      </div>
      <div
        ref={tip}
        aria-hidden
        className={cn(
          "pointer-events-none absolute top-0 left-0 z-10 w-max max-w-64 rounded-control border border-border bg-popover px-2.5 py-2 text-[11px] shadow-sm",
          hoveredTile ? "visible" : "invisible"
        )}
      >
        {hoveredTile && <TileTip tile={hoveredTile} folder={subtree.folder} />}
      </div>
    </div>
  );
});

function placeTip(el: HTMLDivElement | null, e: React.PointerEvent) {
  if (!el) return;
  const box = e.currentTarget.getBoundingClientRect();
  const x = e.clientX - box.left;
  const y = e.clientY - box.top;
  const flipX = x > box.width - 260;
  const flipY = y > box.height - 96;
  el.style.transform = `translate(${flipX ? x - 12 : x + 14}px, ${flipY ? y - 12 : y + 16}px) translate(${flipX ? "-100%" : "0"}, ${flipY ? "-100%" : "0"})`;
}

/** A motion token in ms from the root: 0 under Reduce Motion and Performance mode. */
function tokenMs(el: Element, name: string): number {
  const v = getComputedStyle(el.ownerDocument.documentElement).getPropertyValue(
    name
  );
  const ms = Number.parseFloat(v);
  return Number.isFinite(ms) ? ms : 0;
}

/**
 * Opening a folder grows the new map out of the tile that was clicked (or
 * the row's tile); going up shrinks the old folder back into its tile in
 * the parent's map. A cross-fade when neither map holds the other.
 */
function useZoom(
  layer: RefObject<HTMLDivElement | null>,
  folder: number,
  tiles: readonly SpaceTile[],
  width: number,
  height: number
) {
  const prev = useRef<{ folder: number; tiles: readonly SpaceTile[] } | null>(
    null
  );
  useLayoutEffect(() => {
    const before = prev.current;
    prev.current = { folder, tiles };
    const el = layer.current;
    if (!before || before.folder === folder || !el || width <= 0) return;
    const easing =
      getComputedStyle(el).getPropertyValue("--ease-out") || "ease-out";
    const from = zoomFrom(before, folder, tiles, width, height);
    if (from === null) {
      const ms = tokenMs(el, "--motion-crossfade");
      if (ms > 0)
        el.animate([{ opacity: 0 }, { opacity: 1 }], { duration: ms, easing });
      return;
    }
    const ms = tokenMs(el, "--motion-enter");
    if (ms > 0)
      el.animate(
        [
          { transform: from, opacity: 0.4 },
          { transform: "none", opacity: 1 },
        ],
        { duration: ms, easing }
      );
  }, [layer, folder, tiles, width, height]);
}

/** Where the new map starts: the tile it grows from, or scaled up out of. */
function zoomFrom(
  before: { folder: number; tiles: readonly SpaceTile[] },
  folder: number,
  tiles: readonly SpaceTile[],
  width: number,
  height: number
): string | null {
  const holds = (t: SpaceTile, id: number) =>
    t.node?.id === id || t.chain.some((c) => c.id === id);
  const into = before.tiles.find((t) => holds(t, folder));
  if (into)
    return `translate(${into.x}px, ${into.y}px) scale(${into.w / width}, ${into.h / height})`;
  const outOf = tiles.find((t) => holds(t, before.folder));
  if (outOf && outOf.w > 0 && outOf.h > 0)
    return `scale(${width / outOf.w}, ${height / outOf.h}) translate(${-outOf.x}px, ${-outOf.y}px)`;
  return null;
}

function mapLabel(subtree: SpaceSubtree): string {
  const top = subtree.children
    .filter((c) => c.done && c.kind !== "smaller")
    .slice(0, 3)
    .map((c) => `${c.name} ${formatBytes(c.bytes)}`);
  const head = `Treemap of ${subtree.folder.name}, ${formatBytes(subtree.total_bytes)}`;
  return top.length > 0 ? `${head}. Largest: ${top.join(", ")}.` : `${head}.`;
}

function fill(t: SpaceTile, active: boolean): CSSProperties {
  if (t.frame) return { background: ramp(active ? 3 : 4) };
  if (t.live) return { background: ramp(active ? 3 : 4) };
  switch (t.kind) {
    case "folder":
      return {
        background: ramp(active ? 2 : 3),
        // Folder versus file is shape: a top band on folders at the top level.
        boxShadow: t.depth === 1 ? `inset 0 2px 0 ${ramp(2)}` : undefined,
      };
    case "file":
      return { background: ramp(active ? 2 : 3) };
    case "smaller":
      return { background: ramp(active ? 3 : 4) };
    case "volume":
      return { background: "var(--color-track)" };
    default:
      return { background: HATCH };
  }
}

/**
 * What clicking a tile opens: a frame opens the folder its entries belong to
 * (riley, for Users › riley), a nested folder opens itself, and any other
 * nested entry opens its frame.
 */
function target(t: SpaceTile): SpaceNode | null {
  if (t.frame) {
    const deepest = t.chain.at(-1) ?? null;
    if (canOpen(deepest)) return deepest;
  }
  if (canOpen(t.node)) return t.node;
  return t.parent ? target(t.parent) : null;
}

/** A frame's name: the folder, and any subfolder holding nearly all of it. */
function frameName(t: SpaceTile): string {
  return [t.node?.name ?? "", ...t.chain.map((c) => c.name)].join(" › ");
}

const Tile = memo(function Tile({
  tile: t,
  active,
  onHover,
  onOpen,
}: {
  tile: SpaceTile;
  active: boolean;
  onHover: (key: string | null) => void;
  onOpen: (node: SpaceNode) => void;
}) {
  const opens = target(t) !== null;
  const gap = t.kind === "other" || t.kind === "unmeasured";
  return (
    <div
      aria-hidden
      data-tile={t.key}
      data-kind={t.kind}
      data-depth={t.depth}
      data-live={t.live || undefined}
      className={cn(
        "absolute overflow-hidden rounded-[2px] transition-[background-color] duration-(--motion-fast)",
        // Padding would widen a sliver past its own box.
        t.w < 24 || t.h < 16
          ? "p-0"
          : t.frame
            ? "px-2"
            : t.depth === 2
              ? "px-1.5 py-1"
              : "px-2 py-1.5",
        opens ? "cursor-pointer" : "cursor-default",
        active && "outline outline-1 outline-ring -outline-offset-1",
        gap && "border border-border-strong border-dashed"
      )}
      style={{
        left: t.x,
        top: t.y,
        width: t.w,
        height: t.h,
        ...fill(t, active),
      }}
      onPointerEnter={() => onHover(t.key)}
      onClick={(e) => {
        // A nested tile sits inside its frame; only the innermost acts.
        e.stopPropagation();
        const node = target(t);
        if (node) onOpen(node);
      }}
    >
      {t.frame ? <FrameHeader tile={t} /> : <TileText tile={t} />}
    </div>
  );
});

function FrameHeader({ tile: t }: { tile: SpaceTile }) {
  const size = t.live ? `${formatBytes(t.bytes)} so far` : formatBytes(t.bytes);
  const name = frameName(t);
  const both = t.w >= labelWidth(name, 12) + labelWidth(size) + 36;
  return (
    <div
      className="flex min-w-0 items-center gap-1.5"
      style={{ height: FRAME_HEADER }}
    >
      <span className="min-w-0 truncate font-[590] text-[12px] text-foreground leading-4">
        {name}
      </span>
      {both && (
        <span className="figures shrink-0 text-[11px] text-fg-subtle leading-4">
          {size}
        </span>
      )}
      <ChevronRight
        aria-hidden
        className="ml-auto size-3 shrink-0 text-fg-subtle"
      />
    </div>
  );
}

function TileText({ tile: t }: { tile: SpaceTile }) {
  const size = formatBytes(t.bytes);
  const label = tileLabel(t.w, t.h, t.live ? `${size} so far` : size);
  // A merged entry's name is a count; cut short it says nothing.
  const fits = t.kind !== "smaller" || t.w >= labelWidth(tileName(t)) + 16;
  if (label === "none" || !fits) return null;
  const gap = t.kind === "other" || t.kind === "unmeasured";
  const quiet = t.kind === "volume" || gap || t.live;
  const opens = canOpen(t.node);
  const withShare =
    t.depth === 1 && t.w >= labelWidth(`${size} · ${shareLabel(t.share)}`) + 16;
  return (
    <>
      <div className="flex min-w-0 items-start gap-1">
        <span
          className={cn(
            "min-w-0 flex-1 truncate leading-4",
            t.depth === 2 ? "text-[11px]" : "font-[510] text-[12px]",
            quiet ? "text-muted-foreground" : "text-foreground"
          )}
        >
          {tileName(t)}
        </span>
        {opens && t.depth === 1 && label === "full" && (
          <ChevronRight
            aria-hidden
            className="mt-0.5 size-3 shrink-0 text-fg-subtle"
          />
        )}
      </div>
      {label === "full" && (
        <div className="figures whitespace-nowrap text-[11px] text-fg-subtle leading-4">
          {size}
          {t.live ? (
            <span className="text-muted-foreground"> so far</span>
          ) : (
            withShare && (
              <span className="text-muted-foreground">
                {" · "}
                {shareLabel(t.share)}
              </span>
            )
          )}
        </div>
      )}
    </>
  );
}

function tileName(t: SpaceTile): string {
  if (t.frame) return frameName(t);
  if (t.kind === "unmeasured") return "Not yet measured";
  if (t.kind === "other") return "Other";
  return t.node?.name ?? "";
}

function TileTip({ tile: t, folder }: { tile: SpaceTile; folder: SpaceNode }) {
  const items =
    t.kind === "folder" || t.kind === "smaller"
      ? ` · ${itemsLabel(t.node?.items ?? null)}`
      : "";
  const parent = t.parent;
  const share =
    parent && parent.bytes > 0
      ? `${shareLabel(t.bytes / parent.bytes)} of ${parent.node?.name ?? ""}`
      : `${shareLabel(t.share)} of ${folder.name}`;
  const opens = target(t);
  const action =
    opens === null
      ? ""
      : opens === t.node
        ? " · click to open"
        : ` · click to open ${opens.name}`;
  return (
    <>
      <div className="truncate font-[510] text-[12px] text-foreground">
        {tileName(t)}
      </div>
      <div className="figures mt-0.5 text-fg-subtle">
        {formatBytes(t.bytes)}
        {t.live ? " so far" : ` · ${share}`}
        {items}
      </div>
      <div className="mt-0.5 text-muted-foreground">
        {kindLabel(t.kind, t.node)}
        {action}
      </div>
    </>
  );
}
