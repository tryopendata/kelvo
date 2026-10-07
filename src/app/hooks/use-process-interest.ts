import {
  FULL_TABLE,
  type ProcessSort,
  type ProcessView,
} from "@core/process-interest";
import { useEffect, useState } from "react";
import type {
  ProcessColumn,
  ProcessSort as TableSort,
} from "~/components/process-table";
import { useHost, useHostStore } from "~/stores/host-store";

/**
 * Ask Rust for the process rows the calling component shows while it is
 * mounted (`set_process_interest`, plan 6.3, D-066). Each consumer passes
 * its own view; the window's `ProcessInterest` sends the union of all of
 * them, tied to the live subscription's stream, so a page reload or
 * resubscribe never leaves interest behind. Rust drops it while the window
 * is hidden and restores it when shown. `null` asks for nothing.
 */
export function useProcessInterest(view: ProcessView | null = FULL_TABLE) {
  const interest = useHostStore().getState().processInterest;
  const [id] = useState(() => Symbol("process-interest"));
  // By value: a view rebuilt each render with the same fields is no change.
  const key = view === null ? null : JSON.stringify(view);
  useEffect(() => {
    if (key === null) return;
    interest.set(id, JSON.parse(key) as ProcessView);
    return () => interest.remove(id);
  }, [interest, id, key]);
}

/** Table columns Rust can rank by (`ProcessSort`); the rest need every row. */
const RUST_SORT: Partial<Record<ProcessColumn, ProcessSort>> = {
  cpu: "cpu",
  mem: "memory",
  threads: "threads",
  wakeups: "wakeups",
  energy: "energy",
  diskRead: "disk_read",
  diskWrite: "disk_write",
  diskTotal: "disk_total",
  netRx: "net_rx",
  netTx: "net_tx",
  netTotal: "net_total",
  gpu: "gpu",
};

const NETWORK_SORTS: ReadonlySet<ProcessSort> = new Set([
  "net_rx",
  "net_tx",
  "net_total",
]);

/**
 * The view for a table that shows its top `rows` in `sort` order: the top
 * `rows` by that key when Rust can rank by it descending, otherwise the
 * full table (an ascending sort or a name column needs every row). Ranking
 * by a network key asks for network rates; `network` asks for them anyway
 * (a table that shows the columns under another sort).
 */
export function topRowsView(
  sort: TableSort,
  rows: number,
  network = false,
  gpu = false
): ProcessView {
  const key = RUST_SORT[sort.by];
  const base: ProcessView =
    !key || sort.dir !== "desc"
      ? FULL_TABLE
      : { limit: rows, sort: [key], period_ms: null };
  const withNet =
    network || (key !== undefined && NETWORK_SORTS.has(key))
      ? { ...base, network: true }
      : base;
  return gpu || key === "gpu" ? { ...withNet, gpu: true } : withNet;
}

/**
 * Whether process rows can carry network rates (`process_network`, D-081).
 * False until capabilities arrive and when NetworkStatistics is missing:
 * the network columns and lists stay hidden.
 */
export function useProcessNetwork(): boolean {
  return useHost((s) => s.capabilities?.process_network === true);
}

/**
 * Whether process rows can carry GPU time (`process_gpu`, D-085). False
 * until capabilities arrive and when the GPU's registry clients cannot be
 * read: the GPU columns and lists stay hidden.
 */
export function useProcessGpu(): boolean {
  return useHost((s) => s.capabilities?.process_gpu === true);
}

/** The latest process rows, or `null` before the first batch. */
export function useProcessRows() {
  return useHost((s) => s.processes?.rows ?? null);
}
