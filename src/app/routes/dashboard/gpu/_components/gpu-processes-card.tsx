import type { LiveProcess } from "@core/generated/bindings";
import { useMemo, useState } from "react";
import {
  type ProcessColumn,
  type ProcessSort,
  ProcessTable,
} from "~/components/process-table";
import { SectionCard } from "~/components/section-card";
import {
  topRowsView,
  useProcessInterest,
  useProcessRows,
} from "~/hooks/use-process-interest";
import { measuredGpu } from "../_lib/gpu";

/** Plan 4.8: process, PID, GPU time %, user, sorted by GPU. */
const COLUMNS: readonly ProcessColumn[] = ["name", "pid", "gpu", "user"];

/**
 * Rows shown, and asked for: the top 12 by the table's sort, every sample
 * while the page is visible. Rust converts only the picked rows (D-066).
 */
const ROWS = 12;

const ROW_PX = 29;

/**
 * GPU time per process (D-085). Rendered only when the host has
 * `process_gpu`; it asks for GPU time while mounted. Rows without a measured
 * share (the baseline sample) and rows with none are left out.
 */
export function GpuProcessesCard() {
  const [sort, setSort] = useState<ProcessSort>({ by: "gpu", dir: "desc" });
  useProcessInterest(topRowsView(sort, ROWS, false, true));
  const live = useProcessRows();
  const rows: readonly LiveProcess[] = useMemo(
    () => (live === null ? [] : measuredGpu(live)),
    [live]
  );
  const measured = live?.some((p) => p.gpu_pct !== null) ?? false;

  return (
    <SectionCard
      accent="gpu"
      origin="tr"
      title="Processes"
      variant="default"
      aside={
        <p className="m-0 font-normal text-[11px] text-muted-foreground">
          Your processes only. Share of the whole GPU.
        </p>
      }
    >
      <ProcessTable
        rows={rows}
        columns={COLUMNS}
        sort={sort}
        onSort={setSort}
        height={ROWS * ROW_PX}
        growOnly
        empty={
          measured
            ? "None of your processes used the GPU."
            : "Measuring GPU by process…"
        }
        ariaLabel="GPU by process"
      />
      <p className="m-0 font-normal text-[11px] text-muted-foreground">
        Approximate while a long GPU compute job runs: its time is counted when
        each batch of GPU work finishes.
      </p>
    </SectionCard>
  );
}
