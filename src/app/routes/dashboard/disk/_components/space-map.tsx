import { formatBytes } from "@core/format";
import type { SpaceNode, SpaceSubtree } from "@core/generated/bindings";
import {
  layoutSpace,
  type SpaceTile,
  spaceEntries,
  spaceOrder,
  tileLabel,
} from "@core/space-treemap";
import { ChevronRight } from "lucide-react";
import { type CSSProperties, memo, useMemo, useRef, useState } from "react";
import { useElementSize } from "~/hooks/use-element-size";
import { Swap } from "~/lib/motion/swap";
import { cn } from "~/lib/utils";
import { canOpen, itemsLabel, kindLabel, shareLabel } from "../_lib/space";

const HATCH =
  "repeating-linear-gradient(135deg,var(--color-grid) 0 1px,transparent 1px 7px)";
const ramp = (step: 2 | 3 | 4) =>
  `color-mix(in srgb, var(--color-disk) var(--ramp-${step}), transparent)`;

/**
 * The treemap of one folder (plan/disk-space-report.md "The treemap"). Tiles
 * are the folder's direct children, area by allocated bytes; color carries
 * structure only. Tiles are `aria-hidden`: the table beside it is the
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
  const height = Math.round(Math.min(360, Math.max(220, width * 0.42)));
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
      onPointerMove={(e) => {
        const box = e.currentTarget.getBoundingClientRect();
        const el = tip.current;
        if (!el) return;
        const x = e.clientX - box.left;
        const y = e.clientY - box.top;
        const flipX = x > box.width - 240;
        const flipY = y > box.height - 84;
        el.style.transform = `translate(${flipX ? x - 12 : x + 14}px, ${flipY ? y - 12 : y + 16}px) translate(${flipX ? "-100%" : "0"}, ${flipY ? "-100%" : "0"})`;
      }}
      onPointerEnter={() => setInside(true)}
      onPointerLeave={() => {
        setInside(false);
        onHover(null);
      }}
    >
      <Swap k={folderId} className="absolute inset-0 block">
        {tiles.map((t) => (
          <Tile
            key={t.key}
            tile={t}
            active={t.key === hovered}
            onHover={onHover}
            onOpen={onOpen}
          />
        ))}
      </Swap>
      <div
        ref={tip}
        aria-hidden
        className={cn(
          "pointer-events-none absolute top-0 left-0 z-10 w-max max-w-60 rounded-control border border-border bg-popover px-2.5 py-2 text-[11px] shadow-sm",
          hoveredTile ? "visible" : "invisible"
        )}
      >
        {hoveredTile && <TileTip tile={hoveredTile} />}
      </div>
    </div>
  );
});

function mapLabel(subtree: SpaceSubtree): string {
  const top = subtree.children
    .filter((c) => c.done && c.kind !== "smaller")
    .slice(0, 3)
    .map((c) => `${c.name} ${formatBytes(c.bytes)}`);
  const head = `Treemap of ${subtree.folder.name}, ${formatBytes(subtree.total_bytes)}`;
  return top.length > 0 ? `${head}. Largest: ${top.join(", ")}.` : `${head}.`;
}

function tileStyle(t: SpaceTile, active: boolean): CSSProperties {
  const box = { left: t.x, top: t.y, width: t.w, height: t.h };
  switch (t.kind) {
    case "folder":
      return {
        ...box,
        background: ramp(active ? 2 : 3),
        boxShadow: `inset 0 2px 0 ${ramp(2)}`,
      };
    case "file":
      return { ...box, background: ramp(active ? 2 : 3) };
    case "smaller":
      return { ...box, background: ramp(active ? 3 : 4) };
    case "volume":
      return { ...box, background: "var(--color-track)" };
    default:
      return { ...box, background: HATCH };
  }
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
  const opens = canOpen(t.node);
  const label = tileLabel(t.w, t.h);
  const gap = t.kind === "other" || t.kind === "unmeasured";
  return (
    <div
      aria-hidden
      data-tile={t.key}
      data-kind={t.kind}
      className={cn(
        "absolute overflow-hidden rounded-[2px] px-2 py-1.5 transition-[background-color] duration-(--motion-fast)",
        opens ? "cursor-pointer" : "cursor-default",
        active && "outline outline-1 outline-ring -outline-offset-1",
        gap && "border border-border-strong border-dashed"
      )}
      style={tileStyle(t, active)}
      onPointerEnter={() => onHover(t.key)}
      onClick={() => {
        if (canOpen(t.node)) onOpen(t.node);
      }}
    >
      {label !== "none" && (
        <div className="flex min-w-0 items-start gap-1">
          <span
            className={cn(
              "min-w-0 flex-1 truncate font-[510] text-[12px] leading-4",
              t.kind === "volume" || gap
                ? "text-muted-foreground"
                : "text-foreground"
            )}
          >
            {t.kind === "unmeasured" ? "Not yet measured" : tileName(t)}
          </span>
          {opens && label === "full" && (
            <ChevronRight
              aria-hidden
              className="mt-0.5 size-3 shrink-0 text-fg-subtle"
            />
          )}
        </div>
      )}
      {label === "full" && (
        <div className="figures truncate text-[11px] text-fg-subtle leading-4">
          {formatBytes(t.bytes)}
          <span className="text-muted-foreground">
            {" · "}
            {shareLabel(t.share)}
          </span>
        </div>
      )}
    </div>
  );
});

function tileName(t: SpaceTile): string {
  if (t.kind === "other") return "Other";
  return t.node?.name ?? "";
}

function TileTip({ tile: t }: { tile: SpaceTile }) {
  const name =
    t.kind === "unmeasured" ? "Not yet measured" : (t.node?.name ?? "");
  const items =
    t.kind === "folder" || t.kind === "smaller"
      ? ` · ${itemsLabel(t.node?.items ?? null)}`
      : "";
  return (
    <>
      <div className="truncate font-[510] text-[12px] text-foreground">
        {name}
      </div>
      <div className="figures mt-0.5 text-fg-subtle">
        {formatBytes(t.bytes)} · {shareLabel(t.share)}
        {items}
      </div>
      <div className="mt-0.5 text-muted-foreground">
        {kindLabel(t.kind, t.node)}
        {canOpen(t.node) ? " · click to open" : ""}
      </div>
    </>
  );
}
