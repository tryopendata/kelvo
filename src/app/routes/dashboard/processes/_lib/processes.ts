import { matchProcessQuery } from "@core/app-search";
import { countNoun, formatInteger } from "@core/format";
import type { LiveProcess } from "@core/generated/bindings";
import type { ProcessColumn, ProcessSort } from "~/components/process-table";

/** The column-set control (plan 4.14); Network and GPU from v1.2. */
export type ColumnSet =
  | "cpu"
  | "memory"
  | "energy"
  | "disk"
  | "network"
  | "gpu";

const ALL_SETS: { value: ColumnSet; label: string }[] = [
  { value: "cpu", label: "CPU" },
  { value: "memory", label: "Memory" },
  { value: "energy", label: "Energy" },
  { value: "disk", label: "Disk" },
  { value: "network", label: "Network" },
  { value: "gpu", label: "GPU" },
];

/**
 * The sets the control offers: Network only when the host can attribute
 * traffic to processes (`process_network`, D-081), GPU only when it can
 * attribute GPU time (`process_gpu`, D-085).
 */
export function columnSetOptions(
  processNetwork: boolean,
  processGpu: boolean
): { value: ColumnSet; label: string }[] {
  return ALL_SETS.filter(
    (s) =>
      (s.value !== "network" || processNetwork) &&
      (s.value !== "gpu" || processGpu)
  );
}

/** Visible columns and the default sort of each set; every set shows ports. */
export const COLUMN_SETS: Record<
  ColumnSet,
  { columns: ProcessColumn[]; sort: ProcessSort }
> = {
  cpu: {
    columns: ["name", "pid", "port", "cpu", "mem", "threads", "user"],
    sort: { by: "cpu", dir: "desc" },
  },
  memory: {
    columns: [
      "name",
      "pid",
      "port",
      "mem",
      "compressed",
      "threads",
      "cpu",
      "user",
    ],
    sort: { by: "mem", dir: "desc" },
  },
  energy: {
    columns: ["name", "pid", "port", "energy", "cpu", "user"],
    sort: { by: "energy", dir: "desc" },
  },
  disk: {
    columns: [
      "name",
      "pid",
      "port",
      "diskRead",
      "diskWrite",
      "diskTotal",
      "user",
    ],
    sort: { by: "diskTotal", dir: "desc" },
  },
  network: {
    columns: ["name", "pid", "port", "netRx", "netTx", "netTotal", "user"],
    sort: { by: "netTotal", dir: "desc" },
  },
  gpu: {
    columns: ["name", "pid", "port", "gpu", "cpu", "mem", "user"],
    sort: { by: "gpu", dir: "desc" },
  },
};

/** The footer note while the Network set shows (D-081). */
export const NETWORK_COVERAGE =
  "Network rates cover your processes only; traffic from system daemons is not attributed.";

/** The footer note while the GPU set shows (D-085). */
export const GPU_COVERAGE =
  "GPU shares cover your processes only and are approximate while a long GPU compute job runs.";

/**
 * Client-side search (plan 4.14): a case-insensitive substring of the name,
 * or a PID or listening port that starts with the digits typed. Blank keeps
 * every row.
 */
export function filterProcesses(
  rows: readonly LiveProcess[],
  query: string
): readonly LiveProcess[] {
  const matches = matchProcessQuery(query);
  return matches ? rows.filter(matches) : rows;
}

export function threadCount(rows: readonly LiveProcess[]): number {
  let n = 0;
  for (const p of rows) n += p.threads;
  return n;
}

/** Footer line: "312 processes · 2,104 threads", or "4 of 312" while searching. */
export function countLine(
  all: readonly LiveProcess[],
  shown: readonly LiveProcess[]
): string {
  const n = formatInteger(all.length);
  const procs =
    shown.length === all.length
      ? countNoun(all.length, "process", "processes")
      : `${formatInteger(shown.length)} of ${n} processes`;
  return `${procs} · ${formatInteger(threadCount(shown))} threads`;
}

/**
 * D-045: without root, macOS only lets Kelvo read the user's own processes.
 * The count comes from the collector once the schema carries it; until then
 * the note says it without a number.
 */
export function hiddenNote(hidden: number | null): string {
  if (hidden === null) {
    return "Some processes owned by other users are hidden: macOS only lets administrators read them.";
  }
  if (hidden === 0) return "";
  return `${countNoun(hidden, "process", "processes")} hidden: owned by other users, readable only by administrators.`;
}
