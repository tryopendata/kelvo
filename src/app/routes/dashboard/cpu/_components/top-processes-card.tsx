import { useId, useState } from "react";
import { useNavigate } from "react-router";
import {
  type ProcessSort,
  ProcessTable,
  sortProcesses,
} from "~/components/process-table";
import {
  topRowsView,
  useProcessInterest,
  useProcessRows,
} from "~/hooks/use-process-interest";
import { useUnits } from "~/hooks/use-units";
import { Card } from "~/widgets/card";

const COLUMNS = [
  "name",
  "pid",
  "cpu",
  "threads",
  "wakeups",
  "energy",
  "user",
] as const;
const ROWS = 8;
/** The bar fills at 140% of one core. */
const BAR_MAX_PCT = 140;

/**
 * The top 8 processes by the chosen column, with "Show
 * all" to the Processes page. Asks Rust for process rows while mounted.
 */
export function TopProcessesCard({ coreCount }: { coreCount: number | null }) {
  const titleId = useId();
  const navigate = useNavigate();
  const units = useUnits();
  const [sort, setSort] = useState<ProcessSort>({ by: "cpu", dir: "desc" });
  useProcessInterest(topRowsView(sort, ROWS));
  const rows = useProcessRows();
  const top = rows ? sortProcesses(rows, sort).slice(0, ROWS) : [];

  return (
    <Card accent="cpu" labelledBy={titleId} className="flex flex-col">
      <div className="flex flex-wrap items-center gap-3 px-4 pt-3.5 pb-1.5">
        <h2 id={titleId} className="m-0 flex-1 font-[590] text-[14px]">
          Top processes
        </h2>
        {coreCount !== null && (
          <span className="font-normal text-[11px] text-muted-foreground">
            % CPU is of one core; <span className="data-mono">{coreCount}</span>{" "}
            cores = <span className="data-mono">{coreCount * 100}%</span>
          </span>
        )}
        <button
          type="button"
          onClick={() => navigate("/dashboard/processes")}
          className="rounded-chip font-normal text-[11px] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
        >
          Show all
        </button>
      </div>
      {rows === null ? (
        <p className="m-0 px-4 pb-4 font-normal text-[12px] text-muted-foreground">
          Waiting for the first process sample
        </p>
      ) : (
        <ProcessTable
          rows={top}
          columns={COLUMNS}
          sort={sort}
          onSort={setSort}
          cpuBarMaxPct={BAR_MAX_PCT}
          height={ROWS * 29}
          memUnits={units.bytes}
          rateUnits={units.rate}
          ariaLabel="Top processes by CPU"
        />
      )}
    </Card>
  );
}
