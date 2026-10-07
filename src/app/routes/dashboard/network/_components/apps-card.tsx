import type { TimeRange } from "@core/brush";
import {
  formatBytes,
  formatClockSeconds,
  formatRate,
  MISSING,
  type RateUnits,
} from "@core/format";
import type { NetworkByApp } from "@core/generated/bindings";
import { networkByAppFailure } from "@core/history-state";
import { CommandFailure } from "@core/transport";
import { type ReactNode, useMemo, useState } from "react";
import {
  APP_CELL,
  AppsNote,
  AppsShell,
  ShareCell,
  SortHead,
} from "~/components/app-table";
import { SelectionChip } from "~/components/selection-chip";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "~/components/ui/table";
import { heldFor, useOpenEdge, useRangeScope } from "~/hooks/use-range-scope";
import { useUnits } from "~/hooks/use-units";
import { cn } from "~/lib/utils";
import { InitialChip } from "~/widgets/initial-chip";
import {
  useCompleteEdge,
  useLatestBucket,
  useNetworkByApp,
} from "../_hooks/use-network-by-app";
import {
  type AppRow,
  type AppSort,
  appRows,
  appsTitle,
  firstRecordedMs,
  nowRates,
  openTailS,
  partialCoverage,
  remainderRows,
  sortApps,
  unrecorded,
} from "../_lib/apps";

/**
 * Bytes per app over the selection, or over the chart window (D-091) when
 * nothing is selected (D-089). Totals come from `query_network_by_app`; "now" is
 * the latest complete 10 s bucket of the same command, so its names match.
 * The window ends where the engine's complete buckets end, not at the
 * newest sample: the bucket before it may still be waiting on the per-app
 * stream, and counting it would push its interface bytes into "System and
 * other".
 */
export function AppsCard({ windowMs }: { windowMs: number }) {
  const complete = useCompleteEdge(useOpenEdge());
  const { selection, range, keepPrevious } = useRangeScope(windowMs, complete);
  const totals = useNetworkByApp(range, { keepPrevious });
  const latest = useLatestBucket(complete);
  const units = useUnits();
  const [sort, setSort] = useState<AppSort>({ by: "total", dir: "desc" });

  const data = heldFor(totals, windowMs);
  const now = useMemo(() => nowRates(latest.data), [latest.data]);
  const rows = useMemo(
    () => (data ? sortApps(appRows(data, now), sort) : []),
    [data, now, sort]
  );
  const rest = useMemo(() => (data ? remainderRows(data) : []), [data]);
  const shown: TimeRange | null = data
    ? { fromMs: data.from_ms, toMs: data.to_ms }
    : selection;

  let body: ReactNode;
  if (totals.isError) {
    const error = totals.error;
    body = (
      <AppsNote>
        {error instanceof CommandFailure
          ? networkByAppFailure(error.error)
          : "Couldn't load app totals."}
      </AppsNote>
    );
  } else if (!data) {
    body = <AppsNote>Loading app totals…</AppsNote>;
  } else if (unrecorded(data)) {
    body = <Unrecorded data={data} edge={complete} />;
  } else {
    const partial = partialCoverage(data);
    const tail = openTailS(data);
    body = (
      <>
        {partial && (
          <AppsNote>
            Measured for <span className="data-mono">{partial.measuredS}</span>{" "}
            of <span className="data-mono">{partial.spanS}</span> s. Network
            history wasn't recording for the rest, so these totals cover{" "}
            <span className="data-mono">{partial.measuredS}</span> s.
          </AppsNote>
        )}
        {tail > 0 && (
          <AppsNote>
            The last <span className="data-mono">{tail}</span> s are still being
            measured. These totals update as they close.
          </AppsNote>
        )}
        {data.clamped && (
          <AppsNote>
            App totals here exceed the interface (bytes counted when their app
            was identified), so System and other reads 0 and shares are of the
            table's total.
          </AppsNote>
        )}
        <AppsTable
          rows={rows}
          rest={rest}
          sort={sort}
          onSort={setSort}
          units={units.rate}
        />
      </>
    );
  }

  return (
    <AppsShell
      title={appsTitle(selection ? (shown ?? selection) : null, windowMs)}
      aside={selection && shown && <SelectionChip range={shown} />}
    >
      {body}
    </AppsShell>
  );
}

/**
 * A range with nothing recorded: a hatched box naming when Network history started, read from
 * the time after the range.
 */
function Unrecorded({
  data,
  edge,
}: {
  data: NetworkByApp;
  edge: number | null;
}) {
  const after =
    edge !== null && edge > data.to_ms
      ? { fromMs: data.to_ms, toMs: edge }
      : null;
  const later = useNetworkByApp(after);
  const started = later.data ? firstRecordedMs(later.data) : null;
  return (
    <div className="px-4 pt-1 pb-3.5">
      <div className="flex h-33 items-center justify-center rounded-lg border border-border-strong/40 border-dashed bg-[repeating-linear-gradient(135deg,var(--color-grid)_0_1px,transparent_1px_7px)]">
        <span className="rounded-md border border-border bg-card px-2 py-1 font-normal text-[11px] text-fg-subtle">
          {started === null ? (
            "No app data for this range"
          ) : (
            <>
              No app data · Network history started{" "}
              <span className="data-mono">{formatClockSeconds(started)}</span>
            </>
          )}
        </span>
      </div>
    </div>
  );
}

function AppIcon({ row }: { row: AppRow }) {
  if (row.kind === "system") {
    return (
      <span
        aria-hidden
        className="inline-block size-4 flex-none rounded-[4px] border border-border-strong/50 border-dashed"
      />
    );
  }
  if (row.kind !== "app") {
    return <span aria-hidden className="inline-block w-4 flex-none" />;
  }
  return <InitialChip text={row.name} />;
}

function AppsTable({
  rows,
  rest,
  sort,
  onSort,
  units,
}: {
  rows: readonly AppRow[];
  rest: readonly AppRow[];
  sort: AppSort;
  onSort: (s: AppSort) => void;
  units: RateUnits;
}) {
  const bytes = (n: number) => formatBytes(n);
  return (
    <div className="overflow-x-auto">
      <Table aria-label="Network by app">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead>App</TableHead>
            <TableHead className="text-right">Down</TableHead>
            <TableHead className="text-right">Up</TableHead>
            <SortHead by="total" label="Total" sort={sort} onSort={onSort} />
            <TableHead className="text-right">Share</TableHead>
            <SortHead by="now" label="Now" sort={sort} onSort={onSort} />
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((r) => (
            <TableRow key={r.key} className="text-fg-subtle">
              <TableCell className={APP_CELL}>
                <span className="inline-flex items-center gap-2 text-foreground">
                  <AppIcon row={r} />
                  {r.name}
                </span>
              </TableCell>
              <TableCell className={cn(APP_CELL, "data-mono text-right")}>
                {bytes(r.rxBytes)}
              </TableCell>
              <TableCell className={cn(APP_CELL, "data-mono text-right")}>
                {bytes(r.txBytes)}
              </TableCell>
              <TableCell
                className={cn(APP_CELL, "data-mono text-right text-foreground")}
              >
                {bytes(r.totalBytes)}
              </TableCell>
              <ShareCell share={r.share} muted={false} />
              <TableCell
                className={cn(
                  APP_CELL,
                  "data-mono text-right",
                  !r.nowBps && "text-fg-faint"
                )}
              >
                {formatRate(r.nowBps, { units })}
              </TableCell>
            </TableRow>
          ))}
          {rest.map((r) => (
            <TableRow
              key={r.key}
              className={cn(
                "text-muted-foreground",
                r.kind === rest[0]?.kind && "border-border border-t",
                r.kind === "system" && "border-b-0"
              )}
            >
              <TableCell className={APP_CELL}>
                <span className="inline-flex items-center gap-2">
                  <AppIcon row={r} />
                  {r.name}
                </span>
              </TableCell>
              <TableCell className={cn(APP_CELL, "data-mono text-right")}>
                {bytes(r.rxBytes)}
              </TableCell>
              <TableCell className={cn(APP_CELL, "data-mono text-right")}>
                {bytes(r.txBytes)}
              </TableCell>
              <TableCell className={cn(APP_CELL, "data-mono text-right")}>
                {bytes(r.totalBytes)}
              </TableCell>
              <ShareCell share={r.share} muted />
              <TableCell
                className={cn(APP_CELL, "data-mono text-right text-fg-faint")}
              >
                {MISSING}
              </TableCell>
            </TableRow>
          ))}
          <TableRow className="hover:bg-transparent">
            <TableCell
              colSpan={6}
              className="whitespace-normal px-3 pt-0 pb-2.5 pl-9 font-normal text-[11px] text-muted-foreground leading-[1.45]"
            >
              macOS services (updates, backups, DNS). Kelvo can't see these
              without a helper.
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>
  );
}
