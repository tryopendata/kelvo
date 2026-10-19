import type { SpaceKind, SpaceNode } from "@core/generated/bindings";
import { hierarchy, treemap, treemapSquarify } from "d3-hierarchy";

/**
 * Treemap layout for the disk space report (plan/disk-space-report.md). Rust
 * decides what each entry is and how big; this file only places rectangles.
 */

/** What a tile is drawn as. `unmeasured` is the part of a scan still running. */
export type SpaceTileKind = SpaceKind | "unmeasured";

export interface SpaceTile {
  /** Stable within one folder: the node id, or the kind and name. */
  key: string;
  kind: SpaceTileKind;
  node: SpaceNode | null;
  bytes: number;
  /** Share of the folder's total, 0 to 1. */
  share: number;
  x: number;
  y: number;
  w: number;
  h: number;
}

export const UNMEASURED_KEY = "unmeasured";

export function spaceKey(node: SpaceNode): string {
  return node.id === null ? `${node.kind}:${node.name}` : String(node.id);
}

interface Entry {
  key: string;
  kind: SpaceTileKind;
  node: SpaceNode | null;
  bytes: number;
}

/**
 * The entries a treemap draws, in layout order. A finished folder is drawn
 * largest first. While a folder is still being scanned, only finished
 * entries get tiles, in the order they first finished (`order`), with the
 * unmeasured remainder last; the total is fixed (used bytes at the root), so
 * squarify lays the earlier rows out the same way each second and new tiles
 * append instead of reshuffling the map.
 */
export function spaceEntries(
  children: readonly SpaceNode[],
  unmeasured: number | null,
  order: readonly string[] = []
): Entry[] {
  const entry = (node: SpaceNode): Entry => ({
    key: spaceKey(node),
    kind: node.kind,
    node,
    bytes: node.bytes ?? 0,
  });
  if (unmeasured === null)
    return children
      .filter((c) => (c.bytes ?? 0) > 0)
      .map(entry)
      .sort((a, b) => b.bytes - a.bytes);
  const done = children.filter((c) => c.done && (c.bytes ?? 0) > 0).map(entry);
  const rank = new Map(order.map((k, i) => [k, i]));
  const known = done
    .filter((e) => rank.has(e.key))
    .sort((a, b) => (rank.get(a.key) ?? 0) - (rank.get(b.key) ?? 0));
  const fresh = done
    .filter((e) => !rank.has(e.key))
    .sort((a, b) => b.bytes - a.bytes);
  const rest: Entry[] =
    unmeasured > 0
      ? [
          {
            key: UNMEASURED_KEY,
            kind: "unmeasured",
            node: null,
            bytes: unmeasured,
          },
        ]
      : [];
  return [...known, ...fresh, ...rest];
}

/** The finish order to keep for the next layout: `entries` minus the remainder. */
export function spaceOrder(entries: readonly Entry[]): string[] {
  return entries.filter((e) => e.kind !== "unmeasured").map((e) => e.key);
}

/**
 * Squarified tiles for `entries` in a `width` by `height` box, with a 1 px
 * gutter and whole-pixel edges. Entries keep their given order.
 */
export function layoutSpace(
  entries: readonly Entry[],
  width: number,
  height: number
): SpaceTile[] {
  const total = entries.reduce((a, e) => a + e.bytes, 0);
  if (total <= 0 || width <= 0 || height <= 0) return [];
  const root = hierarchy<{ entry?: Entry; children?: { entry: Entry }[] }>({
    children: entries.map((entry) => ({ entry })),
  }).sum((d) => d.entry?.bytes ?? 0);
  const laid = treemap<{ entry?: Entry }>()
    .tile(treemapSquarify.ratio(1.2))
    .size([width, height])
    .paddingInner(1)
    .round(true)(root);
  return laid.leaves().flatMap((leaf) => {
    const e = leaf.data.entry;
    if (!e) return [];
    return [
      {
        key: e.key,
        kind: e.kind,
        node: e.node,
        bytes: e.bytes,
        share: e.bytes / total,
        x: leaf.x0,
        y: leaf.y0,
        w: Math.max(0, leaf.x1 - leaf.x0),
        h: Math.max(0, leaf.y1 - leaf.y0),
      },
    ];
  });
}

/** How much of a tile's text fits: name and size, name only, or nothing. */
export function tileLabel(w: number, h: number): "full" | "name" | "none" {
  if (w >= 64 && h >= 40) return "full";
  if (w >= 44 && h >= 20) return "name";
  return "none";
}
