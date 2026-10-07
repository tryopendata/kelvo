import type { ClusterInfo, HostInfo } from "@core/generated/bindings";

export interface ClusterView {
  /** "P" or "E", for the heatmap's cluster gap. */
  letter: "P" | "E";
  cores: string[];
}

/** Clusters in display order, performance first. */
export function clusterViews(topology: readonly ClusterInfo[]): ClusterView[] {
  const letter = (c: ClusterInfo): "P" | "E" =>
    c.kind === "efficiency" ? "E" : "P";
  const sorted = [...topology].sort(
    (a, b) => (letter(a) === "P" ? 0 : 1) - (letter(b) === "P" ? 0 : 1)
  );
  return sorted.map((c) => ({ letter: letter(c), cores: c.cores }));
}

/** "Apple M4 Pro · 10 performance + 4 efficiency cores" parts. */
export function coreCounts(info: HostInfo): {
  performance: number;
  efficiency: number;
} {
  let performance = 0;
  let efficiency = 0;
  for (const c of info.cpu_topology) {
    if (c.kind === "efficiency") efficiency += c.cores.length;
    else performance += c.cores.length;
  }
  return { performance, efficiency };
}
