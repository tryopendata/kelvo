import { scopeWords, type TimeRange } from "@core/brush";
import { formatBytes, formatRate, formatSpan } from "@core/format";
import type { LiveProcess, ProcessUsage } from "@core/generated/bindings";
import { FULL_TABLE } from "@core/process-interest";
import { targetKey } from "@core/process-selection";
import { type ReactNode, useMemo, useState } from "react";
import { Link, useLocation } from "react-router";
import { AppsNote, AppsShell } from "~/components/app-table";
import { brushScopeProps } from "~/components/brush-overlay";
import {
  ProcessSelectionHead,
  ProcessSelectionRow,
  ProcessSelectionSurface,
  SharedProcessActions,
  useProcessSelection,
} from "~/components/process-selection";
import {
  diskTotal,
  frozenOrder,
  sortProcesses,
} from "~/components/process-table";
import { SearchField } from "~/components/search-field";
import { Button } from "~/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "~/components/ui/table";
import {
  UsageAppsCard,
  type UsageTableConfig,
} from "~/components/usage-apps-card";
import { useEdition } from "~/hooks/use-edition";
import { useIsLocalHost } from "~/hooks/use-is-local-host";
import {
  useProcessInterest,
  useProcessRows,
} from "~/hooks/use-process-interest";
import { useUsageByApp } from "~/hooks/use-usage-by-app";
import { useBrushRange } from "~/stores/brush-store";

const EMPTY: readonly LiveProcess[] = [];
export const requestedBytes = (
  p: Pick<ProcessUsage, "read_bytes" | "write_bytes">
) =>
  p.read_bytes === null || p.write_bytes === null
    ? null
    : p.read_bytes + p.write_bytes;

/** Presentation-only ranking; Rust supplies byte totals, rates and coverage. */
export function historicalDiskProcesses(
  apps: { processes: ProcessUsage[] }[]
): ProcessUsage[] {
  return apps
    .flatMap((a) => a.processes)
    .sort(
      (a, b) =>
        (requestedBytes(b) ?? -1) - (requestedBytes(a) ?? -1) ||
        a.pid - b.pid ||
        a.start_time_us - b.start_time_us
    );
}

const APP_CONFIG: UsageTableConfig = {
  by: "disk",
  noun: "Disk",
  accent: "disk",
  columns: [
    { label: "Read", format: (p) => formatBytes(p.read_bytes) },
    { label: "Written", format: (p) => formatBytes(p.write_bytes) },
    {
      label: "Total",
      format: (p) => formatBytes(requestedBytes(p)),
      ranked: true,
    },
  ],
  footnote:
    "Bytes requested by this user's processes across all devices. Caches and filesystem work mean these figures differ from device traffic.",
};

export function DiskProcesses({ windowMs }: { windowMs: number }) {
  const [apps, setApps] = useState(false);
  const viewControl = (
    <Button variant="ghost" size="sm" onClick={() => setApps(!apps)}>
      {apps ? "By process" : "By app"}
    </Button>
  );
  return apps ? (
    <UsageAppsCard
      windowMs={windowMs}
      config={APP_CONFIG}
      viewControl={viewControl}
    />
  ) : (
    <DiskProcessList windowMs={windowMs} viewControl={viewControl} />
  );
}

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
function DiskProcessList({
  windowMs,
  viewControl,
}: {
  windowMs: number;
  viewControl: ReactNode;
}) {
  const range = useBrushRange();
  const local = useIsLocalHost();
  const canSignal = useEdition()?.process_signal === true;
  useProcessInterest(local && range === null ? FULL_TABLE : null);
  const live = useProcessRows();
  const history = useUsageByApp(range, "disk");
  const [query, setQuery] = useState("");
  const [heldOrder, setHeldOrder] = useState<string[] | null>(null);
  const [all, setAll] = useState(false);
  const location = useLocation();
  const current = useMemo(() => {
    const sorted = sortProcesses(live ?? EMPTY, {
      by: "diskTotal",
      dir: "desc",
    });
    return heldOrder ? frozenOrder(sorted, heldOrder) : sorted;
  }, [live, heldOrder]);
  const retained = useMemo(
    () => historicalDiskProcesses(history.data?.apps ?? []),
    [history.data]
  );
  const rows = range === null ? current : retained;
  const filtered = rows.filter((p) =>
    `${p.name} ${p.pid}`.toLowerCase().includes(query.toLowerCase().trim())
  );
  const shown = all || query ? filtered : filtered.slice(0, 10);
  const scope = range ? scopeWords(range, windowMs) : "Live";
  const waiting = range ? history.isPending : live === null;
  const unrecorded = range !== null && history.data?.covered_ms === 0;
  const back = `${location.pathname}${location.search}`;
  return (
    <div {...brushScopeProps} className="contents">
      <AppsShell
        accent="disk"
        title={`Processes on this Mac · ${scope}`}
        aside={
          <>
            {viewControl}
            <SearchField value={query} onChange={setQuery} />
          </>
        }
      >
        {!local ? (
          <AppsNote>
            Process accounting is available on the monitored Mac.
          </AppsNote>
        ) : history.isError && range ? (
          <AppsNote>Couldn't load process I/O for this range.</AppsNote>
        ) : (
          <>
            {range && history.data && history.data.covered_ms > 0 && (
              <p className="px-4 pb-2 text-[11px] text-muted-foreground">
                Averages cover {formatSpan(history.data.covered_ms)} sampled in
                this interval.
              </p>
            )}
            <DiskSelection
              rows={shown}
              allTargets={filtered}
              scopeKey={range ? `${range.fromMs}:${range.toMs}` : "live"}
              enabled={canSignal}
            >
              <div
                className="max-h-96 overflow-auto"
                onPointerEnter={() => setHeldOrder(current.map(targetKey))}
                onPointerLeave={() => setHeldOrder(null)}
                onFocusCapture={() => setHeldOrder(current.map(targetKey))}
                onBlurCapture={(e) => {
                  if (!e.currentTarget.contains(e.relatedTarget))
                    setHeldOrder(null);
                }}
              >
                <Table
                  aria-label={`Disk processes · ${scope}`}
                  className="table-fixed"
                >
                  <TableHeader>
                    <TableRow className="hover:bg-transparent">
                      <ProcessSelectionHead />
                      <TableHead className="w-[38%] px-2 sm:px-3">
                        Process
                      </TableHead>
                      <TableHead className="px-1 text-right sm:px-3">
                        {range ? "Read" : "Read/s"}
                      </TableHead>
                      <TableHead className="px-1 text-right sm:px-3">
                        {range ? "Written" : "Write/s"}
                      </TableHead>
                      <TableHead
                        aria-sort="descending"
                        className="px-2 text-right sm:px-3"
                      >
                        Total{range ? "" : "/s"}
                        {range && (
                          <span className="block font-normal">Avg/s</span>
                        )}
                      </TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <DiskRows shown={shown} range={range} back={back} />
                    {shown.length === 0 && (
                      <TableRow>
                        <TableCell
                          colSpan={canSignal ? 5 : 4}
                          className="px-4 py-3 text-muted-foreground"
                        >
                          {waiting
                            ? "Waiting for process samples…"
                            : unrecorded
                              ? "No process data for this range."
                              : query
                                ? `No process matches “${query.trim()}”.`
                                : "No recorded process I/O in this interval."}
                        </TableCell>
                      </TableRow>
                    )}
                  </TableBody>
                </Table>
              </div>
            </DiskSelection>
            {!query && filtered.length > 10 && (
              <button
                type="button"
                onClick={() => setAll(!all)}
                className="mx-4 my-2 self-start rounded-sm text-[12px] text-fg-subtle outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
              >
                {all ? "Show top 10" : `Show all ${filtered.length} processes`}
              </button>
            )}
            <p className="px-4 py-2 font-normal text-[11px] text-muted-foreground">
              Bytes requested by this user's processes across all devices.
              Caches and filesystem work mean these figures differ from device
              traffic.
            </p>
          </>
        )}
      </AppsShell>
    </div>
  );
}

function DiskSelection({
  rows,
  allTargets,
  scopeKey,
  enabled,
  children,
}: {
  rows: readonly (LiveProcess | ProcessUsage)[];
  allTargets: readonly (LiveProcess | ProcessUsage)[];
  scopeKey: string;
  enabled: boolean;
  children: ReactNode;
}) {
  const descriptors = useMemo(
    () =>
      rows.map((p) => ({
        key: targetKey(p),
        label: `${p.name} (${p.pid})`,
        targets: [p],
      })),
    [rows]
  );
  return enabled ? (
    <ProcessSelectionSurface
      rows={descriptors}
      allTargets={allTargets}
      scopeKey={scopeKey}
    >
      {children}
    </ProcessSelectionSurface>
  ) : (
    children
  );
}

function DiskRows({
  shown,
  range,
  back,
}: {
  shown: (LiveProcess | ProcessUsage)[];
  range: TimeRange | null;
  back: string;
}) {
  const selection = useProcessSelection();
  const byKey = new Map(shown.map((p) => [targetKey(p), p]));
  const ordered = selection
    ? selection.rows.flatMap((r) => {
        const p = byKey.get(r.key);
        return p ? [p] : [];
      })
    : shown;
  return (
    <>
      {ordered.map((p) => {
        const historic = "read_bytes" in p;
        const params = new URLSearchParams({ back });
        if (range) {
          params.set("from", String(range.fromMs));
          params.set("to", String(range.toMs));
        }
        return (
          <ProcessSelectionRow
            key={targetKey(p)}
            rowKey={targetKey(p)}
            label={`${p.name} (${p.pid})`}
            targets={[p]}
            className="h-11"
          >
            <TableCell className="px-2 py-1 sm:px-3">
              <Link
                className="block truncate rounded-sm text-foreground outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
                to={`/dashboard/processes/${p.pid}/${p.start_time_us}?${params}`}
                aria-label={`Inspect ${p.name}, PID ${p.pid}`}
              >
                {p.name}
              </Link>
              <span className="figures block truncate text-[11px] text-muted-foreground">
                {p.pid}
                {historic && !p.running ? " · Exited" : ""}
              </span>
            </TableCell>
            <TableCell className="figures px-1 text-right text-fg-subtle sm:px-3">
              {historic
                ? formatBytes(p.read_bytes)
                : formatRate(p.disk_read_bps)}
            </TableCell>
            <TableCell className="figures px-1 text-right text-fg-subtle sm:px-3">
              {historic
                ? formatBytes(p.write_bytes)
                : formatRate(p.disk_write_bps)}
            </TableCell>
            <TableCell className="figures relative px-2 text-right sm:px-3">
              {historic ? (
                <>
                  {formatBytes(requestedBytes(p))}
                  <span className="block text-[11px] text-muted-foreground">
                    {formatRate(p.total_avg_bps)}
                  </span>
                </>
              ) : (
                formatRate(diskTotal(p))
              )}
              <span className="absolute inset-y-0 right-1 hidden items-center bg-card group-focus-within:flex group-hover:flex">
                <SharedProcessActions targets={[p]} />
              </span>
            </TableCell>
          </ProcessSelectionRow>
        );
      })}
    </>
  );
}
