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
  "mem",
  "compressed",
  "threads",
  "user",
] as const;
const ROWS = 8;

/** Processes by memory footprint (plan 4.9), top 8 with "Show all". */
export function MemoryProcessesCard() {
  const titleId = useId();
  const navigate = useNavigate();
  const units = useUnits();
  const [sort, setSort] = useState<ProcessSort>({ by: "mem", dir: "desc" });
  useProcessInterest(topRowsView(sort, ROWS));
  const rows = useProcessRows();
  const top = rows ? sortProcesses(rows, sort).slice(0, ROWS) : [];

  return (
    <Card accent="mem" labelledBy={titleId} className="flex flex-col">
      <div className="flex flex-wrap items-center gap-3 px-4 pt-3.5 pb-1.5">
        <h2 id={titleId} className="m-0 flex-1 font-[590] text-[14px]">
          Top processes
        </h2>
        <span className="font-normal text-[11px] text-muted-foreground">
          Memory is the physical footprint
        </span>
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
          height={ROWS * 29}
          memUnits={units.bytes}
          rateUnits={units.rate}
          ariaLabel="Top processes by memory"
        />
      )}
    </Card>
  );
}
