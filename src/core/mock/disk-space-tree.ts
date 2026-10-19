import type { SpaceSpec } from "./disk-space-fixture";

/** A node of the mock tree. Folders are listed at `start`, a fraction of the scan. */
export interface Node {
  id: number;
  name: string;
  kind: "folder" | "file" | "smaller";
  bytes: number;
  count: number;
  rel: string;
  parent: Node | null;
  kids: Node[];
  start: number;
  /** Latest `start` in the subtree: the folder is done once the scan passes it. */
  last: number;
}

function generated(spec: Extract<SpaceSpec, { t: "leaf" }>): SpaceSpec[] {
  const names = spec.files ?? ["data.bin", "cache.db", "index.sqlite"];
  const shares = [0.42, 0.21, 0.12, 0.08];
  const files: SpaceSpec[] = names.slice(0, 4).map((name, i) => ({
    t: "file",
    name,
    bytes: spec.bytes * (shares[i] ?? 0.05),
  }));
  const used = files.reduce((a, x) => a + (x.t === "file" ? x.bytes : 0), 0);
  const count = 12 + Math.round(spec.bytes / 1e9);
  return [...files, { t: "smaller", bytes: spec.bytes - used, count }];
}

const join = (parent: Node | null, name: string) =>
  parent === null ? "" : parent.rel ? `${parent.rel}/${name}` : name;

function leafNode(s: SpaceSpec, parent: Node, id: number): Node | null {
  if (s.t !== "file" && s.t !== "smaller") return null;
  return {
    id,
    name: s.t === "file" ? s.name : "",
    kind: s.t,
    bytes: s.bytes,
    count: s.t === "smaller" ? s.count : 1,
    rel: join(parent, s.t === "file" ? s.name : ""),
    parent,
    kids: [],
    start: 0,
    last: 0,
  };
}

/** Builds the tree, leaving out skipped folders (by root-relative path). */
export function build(spec: SpaceSpec, skip: ReadonlySet<string>): Node {
  let next = 0;
  const visit = (s: SpaceSpec, parent: Node | null): Node | null => {
    if (s.t === "file" || s.t === "smaller")
      return parent && leafNode(s, parent, next++);
    const rel = join(parent, s.name);
    if (skip.has(rel)) return null;
    const node: Node = {
      id: next++,
      name: s.name,
      kind: "folder",
      bytes: 0,
      count: 1,
      rel,
      parent,
      kids: [],
      start: 0,
      last: 0,
    };
    for (const k of s.t === "dir" ? s.kids : generated(s)) {
      const child = visit(k, node);
      if (child) node.kids.push(child);
    }
    return node;
  };
  const root = visit(spec, null);
  if (!root) throw new Error("the mock root is a folder and never skipped");
  schedule(root);
  return root;
}

/** Folders are read in preorder; a folder's files count when it is listed. */
function schedule(root: Node) {
  const own = (n: Node) =>
    n.kids.reduce((a, k) => a + (k.kind === "folder" ? 0 : k.bytes), 0);
  const order: Node[] = [];
  const walk = (n: Node) => {
    order.push(n);
    for (const k of n.kids) if (k.kind === "folder") walk(k);
  };
  walk(root);
  const total = order.reduce((a, n) => a + own(n), 0) || 1;
  let done = 0;
  for (const n of order) {
    n.start = done / total;
    done += own(n);
  }
  const last = (n: Node): number => {
    n.last = n.kids.reduce(
      (m, k) => (k.kind === "folder" ? Math.max(m, last(k)) : m),
      n.start
    );
    return n.last;
  };
  last(root);
}

export function* nodes(n: Node): Generator<Node> {
  yield n;
  for (const k of n.kids) yield* nodes(k);
}

export function crumbs(n: Node): { id: number; name: string }[] {
  const out: { id: number; name: string }[] = [];
  for (let c: Node | null = n; c; c = c.parent)
    out.unshift({ id: c.id, name: c.name });
  return out;
}

export function sum(
  spec: SpaceSpec,
  skip: ReadonlySet<string>,
  rel = ""
): number {
  if (spec.t === "file" || spec.t === "smaller") return spec.bytes;
  if (skip.has(rel)) return 0;
  if (spec.t === "leaf") return spec.bytes;
  return spec.kids.reduce(
    (a, k) =>
      a +
      sum(k, skip, k.t === "smaller" ? rel : rel ? `${rel}/${k.name}` : k.name),
    0
  );
}
