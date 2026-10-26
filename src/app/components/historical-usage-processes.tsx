import type { TimeRange } from "@core/brush";
import { recordedProcessStart } from "@core/chart-range";
import type { ByteUnits } from "@core/format";
import type { HistoricalUsageMetric } from "@core/generated/bindings";
import { remoteQueryText } from "@core/remote-errors";
import { APP_CELL } from "~/components/app-table";
import { HistoricalUsageCells } from "~/components/historical-usage-cells";
import { Button } from "~/components/ui/button";
import { TableCell, TableRow } from "~/components/ui/table";
import { useHistoricalProcesses } from "~/hooks/use-historical-usage";

export function HistoricalUsageProcesses({
  range,
  by,
  appName,
  columns,
  units,
}: {
  range: TimeRange;
  by: HistoricalUsageMetric;
  appName: string;
  columns: readonly HistoricalUsageMetric[];
  units: ByteUnits;
}) {
  const query = useHistoricalProcesses(range, by, appName);
  const count = columns.length + 1;
  const pages = query.data?.pages ?? [];
  if (query.isError)
    return (
      <TableRow>
        <TableCell colSpan={count} className={APP_CELL}>
          <span role="alert">
            {remoteQueryText(query.error, "recorded processes") ??
              "This process listing could not be read."}
          </span>{" "}
          <Button
            variant="ghost"
            size="sm"
            onClick={() => void query.restart()}
          >
            Reload processes
          </Button>
        </TableCell>
      </TableRow>
    );
  if (!query.data)
    return (
      <TableRow>
        <TableCell colSpan={count} className={APP_CELL}>
          Loading recorded processes…
        </TableCell>
      </TableRow>
    );
  return (
    <>
      <TableRow>
        <TableCell
          colSpan={count}
          className={`${APP_CELL} whitespace-normal pl-13 text-muted-foreground`}
        >
          Recorded instances in the current interval. RAM peaks need not add to
          the app peak.
          {pages[0]?.network_app_level_only &&
            " Network is recorded at app level only."}
        </TableCell>
      </TableRow>
      {pages
        .flatMap((page) => page.processes)
        .map((process) => (
          <TableRow
            key={`${process.pid}:${process.start_time_us}`}
            className="text-fg-subtle"
          >
            <TableCell className={`${APP_CELL} pl-13`}>
              <span className="inline-flex items-baseline gap-2">
                <span>{process.name}</span>
                <span
                  className="figures text-[11px] text-muted-foreground"
                  title={`Started ${recordedProcessStart(process.start_time_us)}`}
                >
                  PID {process.pid}
                  <span className="sr-only">
                    , started {recordedProcessStart(process.start_time_us)}
                  </span>
                </span>
              </span>
            </TableCell>
            <HistoricalUsageCells
              figures={process.figures}
              columns={columns}
              units={units}
              spanMs={range.toMs - range.fromMs}
            />
          </TableRow>
        ))}
      {query.hasNextPage && (
        <TableRow>
          <TableCell colSpan={count} className={`${APP_CELL} pl-13`}>
            <Button
              variant="ghost"
              size="sm"
              disabled={query.isFetchingNextPage}
              onClick={() => void query.fetchNextPage()}
            >
              {query.isFetchingNextPage
                ? "Loading more processes…"
                : "Load more recorded processes"}
            </Button>
          </TableCell>
        </TableRow>
      )}
    </>
  );
}
