import type {
  CommandError,
  SpaceFile,
  SpaceKind,
  SpaceNode,
  SpacePhase,
  SpaceReport,
  SpaceScanStatus,
  SpaceSubtree,
  SpaceTarget,
} from "@core/generated/bindings";
import type { CommandResult } from "@core/transport";
import type { MockTransport } from "./contracts";
import {
  routeDiskSpace,
  type SpaceMethods,
  type SpaceRemotes,
} from "./disk-space-hosts";
import { build, crumbs, type Node, nodes } from "./disk-space-tree";
import { type MockVolume, mockVolumes } from "./disk-space-volumes";
import type { ScenarioName } from "./fixtures";

/** How long a mock scan takes, start to finish. */
export const MOCK_SCAN_MS = 8000;
const MAX_CHILDREN = 120;
/** Rust's `SPACE_PREVIEW`: entries in a folder entry's preview. */
const SPACE_PREVIEW = 24;
const PREVIEW_CHAIN = 3;
const MIN_SHARE_DIVISOR = 200;

const ok = <T>(data: T): CommandResult<T> => ({ status: "ok", data });
const fail = (message: string): CommandResult<never> => ({
  status: "error",
  error: { kind: "disk_space", message } satisfies CommandError,
});

interface Plan extends MockVolume {
  skipped: string[];
  skip: Set<string>;
  /** Fraction of the scan at which the volume disappears, if it does. */
  ejectAt: number | null;
}

class Scan {
  readonly root: Node;
  readonly byId = new Map<number, Node>();
  readonly used: number;
  phase: SpacePhase = "scanning";
  endedAt: number | null = null;
  error: string | null = null;

  constructor(
    readonly id: number,
    readonly plan: Plan,
    readonly startedAt: number
  ) {
    this.root = build(plan.tree, plan.skip);
    for (const n of nodes(this.root)) this.byId.set(n.id, n);
    this.used = plan.volume.used_bytes ?? 0;
  }

  elapsed(now: number) {
    return (this.endedAt ?? now) - this.startedAt;
  }

  /** How far the walk got, 0 to 1. */
  at(now: number) {
    return Math.min(1, this.elapsed(now) / MOCK_SCAN_MS);
  }

  /** Moves a scan whose time is up to done or failed. */
  settle(now: number) {
    if (this.phase !== "scanning") return;
    const eject = this.plan.ejectAt;
    if (eject !== null && this.at(now) >= eject) {
      this.end("failed", this.startedAt + eject * MOCK_SCAN_MS);
      this.error = `Couldn't finish scanning ${this.plan.volume.name}: the volume was disconnected.`;
    } else if (this.at(now) >= 1) {
      this.end("done", this.startedAt + MOCK_SCAN_MS);
    }
  }

  end(phase: SpacePhase, at: number) {
    this.phase = phase;
    this.endedAt = at;
  }

  listed(n: Node, t: number) {
    return n.kind !== "folder" || n.start <= t;
  }

  visible(n: Node, t: number) {
    return n.parent === null || n.parent.start <= t;
  }

  measured(n: Node, t: number): { bytes: number; items: number } {
    if (n.kind !== "folder") return { bytes: n.bytes, items: n.count };
    if (!this.listed(n, t)) return { bytes: 0, items: 0 };
    let bytes = 0;
    let items = 0;
    for (const k of n.kids) {
      const m = this.measured(k, t);
      bytes += m.bytes;
      items += m.items;
    }
    return { bytes, items };
  }

  view(n: Node, t: number): SpaceNode {
    const m = this.measured(n, t);
    return {
      id: n.id,
      name: n.name,
      kind: n.kind === "file" ? "file" : "folder",
      bytes: m.bytes,
      items: m.items,
      done: t >= n.last,
      openable: n.kind === "folder" && m.bytes > 0,
      children: [],
    };
  }

  /**
   * Rust's breakdown: sorted, finished folders fold small entries into
   * "smaller items", and each folder entry previews its own breakdown.
   */
  breakdown(n: Node, t: number): SpaceNode[] {
    return this.entries(n, t, MAX_CHILDREN).map((k) => {
      const node = k.id === null ? undefined : this.byId.get(k.id);
      return node?.kind === "folder"
        ? { ...k, children: this.preview(node, k.bytes ?? 0, t, PREVIEW_CHAIN) }
        : k;
    });
  }

  /** Rust's preview: a subfolder holding 90% or more previews its own entries. */
  private preview(
    n: Node,
    bytes: number,
    t: number,
    depth: number
  ): SpaceNode[] {
    const kids = this.entries(n, t, SPACE_PREVIEW);
    const first = kids[0];
    const node = first?.id == null ? undefined : this.byId.get(first.id);
    if (
      depth > 1 &&
      first &&
      node?.kind === "folder" &&
      (first.bytes ?? 0) >= bytes * 0.9
    )
      kids[0] = {
        ...first,
        children: this.preview(node, first.bytes ?? 0, t, depth - 1),
      };
    return kids;
  }

  private entries(n: Node, t: number, limit: number): SpaceNode[] {
    const done = t >= n.last;
    const total = this.measured(n, t).bytes;
    const floor = this.used / 1_000_000;
    const kids: SpaceNode[] = [];
    let small = { bytes: 0, items: 0, count: 0 };
    n.kids.forEach((k) => {
      if (!this.visible(k, t)) return;
      if (k.kind === "smaller") {
        small = {
          bytes: small.bytes + k.bytes,
          items: small.items + k.count,
          count: small.count + k.count,
        };
        return;
      }
      kids.push(this.view(k, t));
    });
    kids.sort((a, b) => (b.bytes ?? 0) - (a.bytes ?? 0) || cmp(a.name, b.name));
    const kept = kids.filter(
      (k, i) =>
        i < limit &&
        (!done ||
          ((k.bytes ?? 0) >= floor &&
            (k.bytes ?? 0) * MIN_SHARE_DIVISOR >= total))
    );
    for (const k of kids.filter((k) => !kept.includes(k))) {
      small.bytes += k.bytes ?? 0;
      small.items += k.items ?? 0;
      small.count += 1;
    }
    if (small.bytes > 0)
      kept.push(
        synthetic(
          small.count === 1 ? "1 smaller item" : `${small.count} smaller items`,
          "smaller",
          small.bytes,
          done,
          small.items
        )
      );
    return kept;
  }

  subtree(id: number | null, now: number): SpaceSubtree | null {
    const t = this.at(now);
    const n = this.byId.get(id ?? 0);
    if (n?.kind !== "folder" || !this.visible(n, t)) return null;
    const folder = this.view(n, t);
    const children = this.breakdown(n, t);
    const atRoot = n.parent === null;
    const total = atRoot ? this.used : (folder.bytes ?? 0);
    if (atRoot) {
      for (const sib of this.plan.siblings)
        children.push(synthetic(sib.label, "volume", sib.bytes));
      const other = this.other(t);
      if (folder.done && other !== null && other > 0)
        children.push(
          synthetic(
            "Other: snapshots, APFS metadata, unreadable and skipped folders",
            "other",
            other
          )
        );
      children.sort((a, b) => (b.bytes ?? 0) - (a.bytes ?? 0));
      folder.bytes = total;
    }
    const attributed = children
      .filter((c) => c.done)
      .reduce((a, c) => a + (c.bytes ?? 0), 0);
    return {
      folder,
      path: crumbs(n),
      children,
      unmeasured_bytes: folder.done ? null : Math.max(0, total - attributed),
      total_bytes: total,
    };
  }

  other(t: number): number | null {
    const files = this.measured(this.root, t).bytes;
    const siblings = this.plan.siblings.reduce((a, s) => a + s.bytes, 0);
    const rest = this.used - files - siblings;
    return rest >= 0 ? rest : null;
  }

  largest(now: number): SpaceFile[] {
    const t = this.at(now);
    return [...nodes(this.root)]
      .filter((n) => n.kind === "file" && this.visible(n, t))
      .sort((a, b) => b.bytes - a.bytes)
      .slice(0, 50)
      .map((n, index) => ({
        index,
        name: n.name,
        parent: n.parent?.rel ?? "",
        bytes: n.bytes,
      }));
  }

  /** The path Finder shows: the boot Data volume is firmlinked into "/". */
  path(target: SpaceTarget, now: number): string | null {
    const rel =
      target.kind === "node"
        ? this.byId.get(target.id)?.rel
        : ((f) => f && (f.parent ? `${f.parent}/${f.name}` : f.name))(
            this.largest(now)[target.index]
          );
    if (rel === undefined) return null;
    const mount = this.plan.volume.mount;
    return mount === "/" ? `/${rel}` : `${mount}/${rel}`;
  }

  status(now: number): SpaceScanStatus {
    const t = this.at(now);
    const m = this.measured(this.root, t);
    const reading = [...nodes(this.root)]
      .filter((n) => n.kind === "folder" && n.start <= t)
      .at(-1);
    return {
      phase: this.phase,
      report_id: this.id,
      volume: this.plan.volume.mount,
      elapsed_ms: this.elapsed(now),
      items: m.items,
      measured_bytes: m.bytes,
      used_bytes: this.used,
      current: this.phase === "scanning" ? (reading?.rel ?? "") : null,
      error: null,
    };
  }

  report(): SpaceReport {
    const files = this.measured(this.root, 1).bytes;
    return {
      id: this.id,
      volume: this.plan.volume,
      finished_ms: this.endedAt,
      duration_ms: MOCK_SCAN_MS,
      items: this.measured(this.root, 1).items,
      used_bytes: this.used,
      files_bytes: files,
      volumes_bytes: this.plan.siblings.reduce((a, s) => a + s.bytes, 0),
      other_bytes: this.other(1),
      unreadable: this.plan.unreadable.count,
      unreadable_paths: [...this.plan.unreadable.paths],
      skipped: this.plan.skipped,
      truncated: false,
    };
  }
}

const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

function synthetic(
  name: string,
  kind: SpaceKind,
  bytes: number,
  done = true,
  items = 0
): SpaceNode {
  return {
    id: null,
    name,
    kind,
    bytes,
    items,
    done,
    openable: false,
    children: [],
  };
}

/**
 * This host's space reports, one scan at a time; a remote host's calls go
 * to its own peer (D-117), so each host keeps its own scan and report.
 */
export function createMockDiskSpace(
  scenarios: readonly ScenarioName[],
  now: () => number,
  remotes: SpaceRemotes = new Map()
): SpaceMethods & Pick<MockTransport, "endSpaceSession"> {
  const appstore = scenarios.includes("appstore");
  let nextId = 0;
  let running: Scan | null = null;
  let report: Scan | null = null;
  let ended: Scan | null = null;
  const unavailable = () =>
    fail("Space reports are not available in this edition");
  const settle = () => {
    running?.settle(now());
    if (!running || running.phase === "scanning") return;
    if (running.phase === "done") report = running;
    else ended = running;
    running = null;
  };
  const find = (id: number) =>
    [running, report].find((s) => s !== null && s.id === id) ?? null;
  const gone = fail("this report is no longer available; scan again");
  const idle: SpaceScanStatus = {
    phase: "idle",
    report_id: null,
    volume: null,
    elapsed_ms: 0,
    items: 0,
    measured_bytes: 0,
    used_bytes: 0,
    current: null,
    error: null,
  };
  const status = (): SpaceScanStatus => {
    settle();
    const scan = running ?? ended ?? report;
    if (!scan) return idle;
    if (scan === ended)
      return {
        ...idle,
        phase: scan.phase,
        volume: scan.plan.volume.mount,
        error: scan.error,
      };
    return scan.status(now());
  };
  const routed = routeDiskSpace(
    {
      diskSpaceVolumes: async () =>
        appstore
          ? unavailable()
          : ok(mockVolumes(scenarios).map((v) => v.volume)),
      diskSpaceStart: async (_host, mount, include) => {
        if (appstore) return unavailable();
        settle();
        if (running) return fail("a scan is already running");
        const spec = mockVolumes(scenarios).find(
          (v) => v.volume.mount === mount
        );
        if (!spec) return fail(`${mount} is not a volume Kelvo can scan`);
        const skipped = spec.volume.protected.filter(
          (p) => !include.includes(p.key)
        );
        running = new Scan(
          ++nextId,
          {
            ...spec,
            skipped: skipped.map((p) => p.label),
            skip: new Set(skipped.map((p) => p.key)),
            ejectAt:
              !spec.volume.is_boot && scenarios.includes("disk-space-eject")
                ? 0.4
                : null,
          },
          now()
        );
        ended = null;
        return ok(running.status(now()));
      },
      diskSpaceStatus: async () => ok(status()),
      diskSpaceCancel: async () => {
        settle();
        running?.end("cancelled", now());
        settle();
        return ok(null);
      },
      diskSpaceReport: async () => {
        settle();
        return ok(report?.report() ?? null);
      },
      diskSpaceSubtree: async (_host, id, node) => {
        settle();
        const tree = find(id)?.subtree(node, now());
        return tree ? ok(tree) : gone;
      },
      diskSpaceLargestFiles: async (_host, id) => {
        settle();
        const scan = find(id);
        return scan ? ok(scan.largest(now())) : gone;
      },
      diskSpaceReveal: async (id, target) => {
        settle();
        return find(id)?.path(target, now()) ? ok(null) : gone;
      },
      diskSpacePath: async (_host, id, target) => {
        settle();
        const path = find(id)?.path(target, now());
        return path ? ok(path) : gone;
      },
    },
    remotes
  );
  return {
    ...routed,
    endSpaceSession: () => {
      running?.end("cancelled", now());
      running = null;
      report = null;
      ended = null;
    },
  };
}
