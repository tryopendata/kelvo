import type { LiveProcess } from "@core/generated/bindings";
import { FULL_TABLE, type ProcessView } from "@core/process-interest";
import { useMemo, useState } from "react";
import { PageHeader } from "~/components/page-header";
import { SELECTION_BAR_PX } from "~/components/process-selection";
import { type ProcessSort, ProcessTable } from "~/components/process-table";
import { SearchField } from "~/components/search-field";
import { SectionCard } from "~/components/section-card";
import { SegmentedControl } from "~/components/segmented-control";
import { useEdition } from "~/hooks/use-edition";
import { useElementSize } from "~/hooks/use-element-size";
import { useHostNoun } from "~/hooks/use-host-record";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import {
  useProcessColumnCaps,
  useProcessGpu,
  useProcessInterest,
  useProcessNetwork,
  useProcessRows,
} from "~/hooks/use-process-interest";
import { useLayout } from "~/hooks/use-ring";
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

/**
 * Processes (plan 4.14): the CPU page's "Top processes" table at full
 * height, every process, with search and a column-set control. Row order
 * holds while the pointer is over the table. Quit and Force Quit (D-029) are
 * a hover action and the row's context menu, each behind a confirm dialog.
 */
// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
export default function ProcessesRoute() {
  const local = useIsLocalHost();
  const processNetwork = useProcessNetwork();
  const processGpu = useProcessGpu();
  const caps = useProcessColumnCaps();
  const options = columnSetOptions({
    network: processNetwork,
    gpu: processGpu,
    energy: caps.energy,
    disk: caps.diskIo,
  });
  // A host without compressed memory (Linux, D-115) has no such column.
  const compressed = useLayout()?.byMetric.has("mem.compressed") ?? true;
  // Without root, macOS hides other users' processes; Linux does not (D-045).
  const mac = useHostNoun() === "Mac";
  const [chosen, setChosen] = useState<ColumnSet>("cpu");
  // A set goes away with its capability; fall back to CPU.
  const set = options.some((o) => o.value === chosen) ? chosen : "cpu";
  const columns = compressed
    ? COLUMN_SETS[set].columns
    : COLUMN_SETS[set].columns.filter((c) => c !== "compressed");
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
  // The App Store edition has no Quit or Force Quit (D-065).
  const edition = useEdition();
  const canSignal = local && edition?.process_signal === true;
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<ProcessSort>(COLUMN_SETS.cpu.sort);
  // The table's sort, unless it names a column the shown set does not have.
  const tableSort = columns.includes(sort.by) ? sort : COLUMN_SETS[set].sort;
  const [bodyRef, measured] = useElementSize("height", 480);
  const bodyHeight = Math.floor(measured);

  const shown = useMemo(() => filterProcesses(rows, query), [rows, query]);
  const loading = live === null;
  const coverage =
    set === "network" ? NETWORK_COVERAGE : set === "gpu" ? GPU_COVERAGE : null;
  const hidden = mac ? hiddenNote(null) : "";
  const note = [hidden, coverage].filter(Boolean).join(" ");

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
              options={options}
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
          <ProcessTable
            rows={shown}
            columns={columns}
            sort={tableSort}
            onSort={setSort}
            height={Math.max(
              120,
              bodyHeight - TABLE_CHROME_PX - (canSignal ? SELECTION_BAR_PX : 0)
            )}
            memUnits={units.bytes}
            freezeOrderOnHover
            selectable={canSignal}
            empty={
              loading
                ? "Waiting for the first process sample…"
                : `No process matches “${query.trim()}”.`
            }
            ariaLabel="Processes"
          />
        </div>
        <footer className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-1 border-border-subtle border-t px-4 py-2.5 font-normal text-[11px] text-muted-foreground">
          <span className="figures">{countLine(rows, shown)}</span>
          {note && <span>{note}</span>}
        </footer>
      </SectionCard>
    </div>
  );
}
