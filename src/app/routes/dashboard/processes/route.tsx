import type { LiveProcess } from "@core/generated/bindings";
import { FULL_TABLE, type ProcessView } from "@core/process-interest";
import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { PageHeader } from "~/components/page-header";
import {
  ProcessContextMenu,
  ProcessRowActions,
  QuitDialog,
} from "~/components/process-actions";
import { type ProcessSort, ProcessTable } from "~/components/process-table";
import { SearchField } from "~/components/search-field";
import { SectionCard } from "~/components/section-card";
import { SegmentedControl } from "~/components/segmented-control";
import { useEdition } from "~/hooks/use-edition";
import {
  useProcessGpu,
  useProcessInterest,
  useProcessNetwork,
  useProcessRows,
} from "~/hooks/use-process-interest";
import { useQuitFlow } from "~/hooks/use-quit-flow";
import { useUnits } from "~/hooks/use-units";
import {
  COLUMN_SETS,
  type ColumnSet,
  columnSetOptions,
  countLine,
  filterProcesses,
  GPU_COVERAGE,
  hiddenNote,
  NETWORK_COVERAGE,
} from "./_lib/processes";

/** Header row plus the 4 px slack ProcessTable adds below its body. */
const TABLE_CHROME_PX = 33;

const EMPTY: readonly LiveProcess[] = [];

/** Every process with its listening ports: every column set shows them. */
const FULL_TABLE_PORTS: ProcessView = { ...FULL_TABLE, ports: true };

/** With network rates (D-081): the Network column set. */
const FULL_TABLE_NETWORK: ProcessView = { ...FULL_TABLE_PORTS, network: true };

/** With GPU time (D-085): the GPU column set. */
const FULL_TABLE_GPU: ProcessView = { ...FULL_TABLE_PORTS, gpu: true };

/** Height of the element, tracked as the window resizes. */
function useMeasuredHeight(fallback: number) {
  const ref = useRef<HTMLDivElement>(null);
  const [height, setHeight] = useState(fallback);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([entry]) => {
      const h = Math.floor(entry?.contentRect.height ?? 0);
      if (h > 0) setHeight(h);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  return [ref, height] as const;
}

/**
 * Processes (plan 4.14): the CPU page's "Top processes" table at full
 * height, every process, with search and a column-set control. Row order
 * holds while the pointer is over the table. Quit and Force Quit (D-029) are
 * a hover action and the row's context menu, each behind a confirm dialog.
 */
export default function ProcessesRoute() {
  const processNetwork = useProcessNetwork();
  const processGpu = useProcessGpu();
  const [chosen, setChosen] = useState<ColumnSet>("cpu");
  // The Network and GPU sets go away with their capability; fall back to CPU.
  const set =
    (chosen === "network" && !processNetwork) ||
    (chosen === "gpu" && !processGpu)
      ? "cpu"
      : chosen;
  useProcessInterest(
    set === "network"
      ? FULL_TABLE_NETWORK
      : set === "gpu"
        ? FULL_TABLE_GPU
        : FULL_TABLE_PORTS
  );
  const live = useProcessRows();
  const rows = live ?? EMPTY;
  const units = useUnits();
  const flow = useQuitFlow();
  // The App Store edition has no Quit or Force Quit (D-065).
  const canSignal = useEdition()?.process_signal === true;
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<ProcessSort>(COLUMN_SETS.cpu.sort);
  // The table's sort, unless it names a column the shown set does not have.
  const tableSort = COLUMN_SETS[set].columns.includes(sort.by)
    ? sort
    : COLUMN_SETS[set].sort;
  const [bodyRef, bodyHeight] = useMeasuredHeight(480);

  const shown = useMemo(() => filterProcesses(rows, query), [rows, query]);
  const loading = live === null;
  const coverage =
    set === "network" ? NETWORK_COVERAGE : set === "gpu" ? GPU_COVERAGE : null;
  const note =
    coverage === null ? hiddenNote(null) : `${hiddenNote(null)} ${coverage}`;

  return (
    <div className="flex h-full min-h-[420px] flex-col gap-4">
      <PageHeader
        title="Processes"
        actions={
          <div className="flex items-center gap-3">
            <SearchField
              value={query}
              onChange={setQuery}
              placeholder="Name, PID or port"
            />
            <SegmentedControl
              options={columnSetOptions(processNetwork, processGpu)}
              value={set}
              onChange={(next) => {
                setChosen(next);
                setSort(COLUMN_SETS[next].sort);
              }}
              ariaLabel="Columns"
            />
          </div>
        }
      />
      <SectionCard
        accent="cpu"
        title="All processes"
        hiddenTitle
        flush
        className="min-h-0 flex-1"
      >
        <div ref={bodyRef} className="min-h-0 flex-1 px-1 pt-1">
          {shown.length > 0 ? (
            <ProcessTable
              rows={shown}
              columns={COLUMN_SETS[set].columns}
              sort={tableSort}
              onSort={setSort}
              height={Math.max(120, bodyHeight - TABLE_CHROME_PX)}
              memUnits={units.bytes}
              freezeOrderOnHover
              rowAction={
                canSignal
                  ? (p) => <ProcessRowActions p={p} onRequest={flow.request} />
                  : undefined
              }
              rowContextMenu={
                canSignal
                  ? (p) => <ProcessContextMenu p={p} onRequest={flow.request} />
                  : undefined
              }
              ariaLabel="Processes"
            />
          ) : (
            <p className="px-3 py-6 font-normal text-[12px] text-muted-foreground">
              {loading
                ? "Waiting for the first process sample…"
                : `No process matches “${query.trim()}”.`}
            </p>
          )}
        </div>
        <footer className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 border-border-subtle border-t px-4 py-2.5 font-normal text-[11px] text-muted-foreground">
          <span className="data-mono">{countLine(rows, shown)}</span>
          {note && <span>{note}</span>}
        </footer>
      </SectionCard>
      <QuitDialog flow={flow} />
    </div>
  );
}
