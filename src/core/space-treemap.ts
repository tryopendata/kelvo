import type { SpaceKind, SpaceNode } from "@core/generated/bindings";
import { hierarchy, treemap, treemapSquarify } from "d3-hierarchy";

/**
 * Treemap layout for the disk space report (plan/disk-space-report.md). Rust
 * decides what each entry is and how big; this file only places rectangles.
 */

/** What a tile is drawn as. `unmeasured` is the part of a scan still running. */
export type SpaceTileKind = SpaceKind | "unmeasured";

export interface SpaceTile {
  /** Stable within one folder: the node id, or the kind and name; nested
   * tiles prefix their parent's key ("12/40"). */
  key: string;
  kind: SpaceTileKind;
  node: SpaceNode | null;
  bytes: number;
  /** Share of the map's total, 0 to 1. */
  share: number;
  /** 1 for the folder's entries, 2 for an entry's preview inside its frame. */
  depth: 1 | 2;
  /** The depth-1 tile a nested tile sits in. */
  parent: SpaceTile | null;
  /** A depth-1 folder drawn as a frame around its own entries. */
  frame: boolean;
  /** A folder still being scanned: its size is what was measured so far. */
  live: boolean;
  /** For a frame: the subfolders its preview passes through, when one holds
   * nearly all of it (Users › riley). Its entries are the last one's. */
  chain: SpaceNode[];
  x: number;
  y: number;
  w: number;
  h: number;
}

export const UNMEASURED_KEY = "unmeasured";
/** Height of a frame's name row; its entries sit below it. */
export const FRAME_HEADER = 20;
/** Gutter between a frame's edge and its entries. */
const FRAME_INSET = 2;
/** A frame needs room for its name row and a useful area below it. */
const FRAME_MIN = { w: 72, h: 60 };

export function spaceKey(node: SpaceNode): string {
  return node.id === null ? `${node.kind}:${node.name}` : String(node.id);
}

export interface SpaceEntry {
  key: string;
  kind: SpaceTileKind;
  node: SpaceNode | null;
  bytes: number;
  live: boolean;
  /** Laid out inside this entry's box instead of drawing it: the running part
   * of a scan, folders still being measured and then the unmeasured rest. */
  group?: SpaceEntry[];
}

const entryOf = (node: SpaceNode, live = false): SpaceEntry => ({
  key: spaceKey(node),
  kind: node.kind,
  node,
  bytes: node.bytes ?? 0,
  live,
});

/**
 * The entries a treemap draws, in layout order. A finished folder is drawn
 * largest first. While a folder is still being scanned, finished entries come
 * first in the order they finished (`order`), and the rest of the total is one
 * region holding the folders still being measured (by discovery order, so they
 * grow in place) and then what nobody has reached yet. The total is fixed
 * (used bytes at the root), so squarify lays the finished rows out the same way
 * each second: tiles append instead of reshuffling the map.
 */
export function spaceEntries(
  children: readonly SpaceNode[],
  unmeasured: number | null,
  order: readonly string[] = []
): SpaceEntry[] {
  const sized = children.filter((c) => (c.bytes ?? 0) > 0);
  if (unmeasured === null)
    return sized.map((c) => entryOf(c)).sort((a, b) => b.bytes - a.bytes);
  const done = sized.filter((c) => c.done).map((c) => entryOf(c));
  const rank = new Map(order.map((k, i) => [k, i]));
  const known = done
    .filter((e) => rank.has(e.key))
    .sort((a, b) => (rank.get(a.key) ?? 0) - (rank.get(b.key) ?? 0));
  const fresh = done
    .filter((e) => !rank.has(e.key))
    .sort((a, b) => b.bytes - a.bytes);
  // Running folders count toward the unmeasured total until they finish.
  const live = sized
    .filter((c) => !c.done)
    .sort((a, b) => (a.id ?? 0) - (b.id ?? 0))
    .map((c) => entryOf(c, true));
  const liveBytes = live.reduce((a, e) => a + e.bytes, 0);
  const rest = Math.max(0, unmeasured - liveBytes);
  const group: SpaceEntry[] = [
    ...live,
    ...(rest > 0
      ? [
          {
            key: UNMEASURED_KEY,
            kind: "unmeasured" as const,
            node: null,
            bytes: rest,
            live: false,
          },
        ]
      : []),
  ];
  const region = liveBytes + rest;
  if (region <= 0) return [...known, ...fresh];
  return [
    ...known,
    ...fresh,
    {
      key: UNMEASURED_KEY,
      kind: "unmeasured",
      node: null,
      bytes: region,
      live: false,
      group,
    },
  ];
}

/** The finish order to keep for the next layout: finished entries only. */
export function spaceOrder(entries: readonly SpaceEntry[]): string[] {
  return entries.filter((e) => e.kind !== "unmeasured").map((e) => e.key);
}

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Squarified boxes for `entries` inside `box`, in the given order. */
function squarify(
  entries: readonly SpaceEntry[],
  box: Box
): { entry: SpaceEntry; box: Box }[] {
  const total = entries.reduce((a, e) => a + e.bytes, 0);
  if (total <= 0 || box.w <= 0 || box.h <= 0) return [];
  const root = hierarchy<{ entry?: SpaceEntry; children?: unknown[] }>({
    children: entries.map((entry) => ({ entry })),
  }).sum((d) => d.entry?.bytes ?? 0);
  const laid = treemap<{ entry?: SpaceEntry }>()
    .tile(treemapSquarify.ratio(1.2))
    .size([box.w, box.h])
    .paddingInner(1)
    .round(true)(root);
  return laid.leaves().flatMap((leaf) => {
    const entry = leaf.data.entry;
    if (!entry) return [];
    return [
      {
        entry,
        box: {
          x: box.x + leaf.x0,
          y: box.y + leaf.y0,
          w: Math.max(0, leaf.x1 - leaf.x0),
          h: Math.max(0, leaf.y1 - leaf.y0),
        },
      },
    ];
  });
}

const tileOf = (
  entry: SpaceEntry,
  box: Box,
  total: number,
  parent: SpaceTile | null
): SpaceTile => ({
  key: parent ? `${parent.key}/${entry.key}` : entry.key,
  kind: entry.kind,
  node: entry.node,
  bytes: entry.bytes,
  share: entry.bytes / total,
  depth: parent ? 2 : 1,
  parent,
  frame: false,
  live: entry.live,
  chain: [],
  ...box,
});

/** A finished folder with entries of its own and room to show them. */
function framed(entry: SpaceEntry, box: Box): boolean {
  return (
    entry.kind === "folder" &&
    (entry.node?.children.length ?? 0) > 0 &&
    box.w >= FRAME_MIN.w &&
    box.h >= FRAME_MIN.h
  );
}

/**
 * The entries a frame shows. Rust previews a subfolder's own entries only
 * when it holds nearly all of its folder, so follow those down, keeping the
 * siblings passed on the way so the areas still add up.
 */
export function framePreview(node: SpaceNode): {
  chain: SpaceNode[];
  entries: SpaceNode[];
} {
  const chain: SpaceNode[] = [];
  const passed: SpaceNode[] = [];
  let at = node;
  for (;;) {
    const [first, ...rest] = at.children;
    if (first?.kind !== "folder" || first.children.length === 0) break;
    chain.push(first);
    passed.push(...rest);
    at = first;
  }
  return { chain, entries: [...at.children, ...passed] };
}

/** A frame's own entries, largest first, in the area below its name row. */
function nested(frame: SpaceTile, total: number): SpaceTile[] {
  if (!frame.node) return [];
  const { chain, entries } = framePreview(frame.node);
  frame.chain = chain;
  const inner = {
    x: frame.x + FRAME_INSET,
    y: frame.y + FRAME_HEADER,
    w: frame.w - FRAME_INSET * 2,
    h: frame.h - FRAME_HEADER - FRAME_INSET,
  };
  const kids = entries
    .filter((c) => (c.bytes ?? 0) > 0)
    .map((c) => entryOf(c))
    .sort((a, b) => b.bytes - a.bytes);
  return squarify(kids, inner)
    .filter(({ box }) => box.w >= 2 && box.h >= 2)
    .map(({ entry, box }) => tileOf(entry, box, total, frame));
}

/**
 * Tiles for `entries` in a `width` by `height` box, with a 1 px gutter and
 * whole-pixel edges. Entries keep their given order. A group entry is laid
 * out inside its own box; a finished folder big enough becomes a frame with
 * its own entries inside, so the map shows two levels at once.
 */
export function layoutSpace(
  entries: readonly SpaceEntry[],
  width: number,
  height: number
): SpaceTile[] {
  const total = entries.reduce((a, e) => a + e.bytes, 0);
  const place = (entry: SpaceEntry, box: Box): SpaceTile[] => {
    if (entry.group)
      return squarify(entry.group, box).flatMap((g) =>
        g.box.w >= 2 && g.box.h >= 2 ? place(g.entry, g.box) : []
      );
    const tile = tileOf(entry, box, total, null);
    if (!framed(entry, box)) return [tile];
    tile.frame = true;
    return [tile, ...nested(tile, total)];
  };
  // Rounding leaves some entries without area; they have no tile to draw.
  return squarify(entries, { x: 0, y: 0, w: width, h: height }).flatMap((p) =>
    p.box.w >= 2 && p.box.h >= 2 ? place(p.entry, p.box) : []
  );
}

/** Approximate width of `text` at the tile label size, for fitting labels. */
export function labelWidth(text: string, px = 11): number {
  return Math.ceil(text.length * px * 0.58);
}

/**
 * How much of a tile's text fits: name and size, name only, or nothing. The
 * size line is shown whole or not at all, never cut mid-figure.
 */
export function tileLabel(
  w: number,
  h: number,
  size = ""
): "full" | "name" | "none" {
  if (w >= Math.max(64, labelWidth(size) + 16) && h >= 40) return "full";
  if (w >= 44 && h >= 20) return "name";
  return "none";
}
