import {
  formatBytes,
  formatClockSeconds,
  formatPercent,
  formatRate,
  formatSpan,
} from "@core/format";
import { FULL_TABLE } from "@core/process-interest";
import { Link, useParams, useSearchParams } from "react-router";
import { PageHeader } from "~/components/page-header";
import { diskTotal } from "~/components/process-table";
import { SectionCard } from "~/components/section-card";
import { useChartWindow } from "~/hooks/use-chart-window";
import {
  useProcessInterest,
  useProcessRows,
} from "~/hooks/use-process-interest";
import { useOpenEdge } from "~/hooks/use-range-scope";
import { useUsageByApp } from "~/hooks/use-usage-by-app";
import { requestedBytes } from "../disk/_components/disk-processes";
import { diskRange } from "../disk/_lib/disk-range";

/** Focused I/O inspection, using the same retained batch as the Disk table. */
// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
export default function ProcessDetailRoute() {
  const { pid: rawPid, startTime } = useParams();
  const pid = Number(rawPid);
  const start = Number(startTime);
  const [params] = useSearchParams();
  const selected = diskRange(params);
  const windowMs = useChartWindow()?.windowMs ?? null;
  const edge = useOpenEdge();
  const range =
    selected ??
    (edge !== null && windowMs !== null
      ? { fromMs: edge - windowMs, toMs: edge }
      : null);
  const history = useUsageByApp(range, "disk");
  useProcessInterest(selected ? null : FULL_TABLE);
  const live = useProcessRows()?.find(
    (p) => p.pid === pid && p.start_time_us === start
  );
  const retained = history.data?.apps
    .flatMap((a) => a.processes)
    .find((p) => p.pid === pid && p.start_time_us === start);
  const process = selected ? retained : (live ?? retained);
  const requestedBack = params.get("back");
  const back = requestedBack?.startsWith("/dashboard/disk")
    ? requestedBack
    : "/dashboard/disk";
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Link
        to={back}
        className="self-start rounded-sm text-[12px] text-fg-subtle outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
      >
        Back to Disk
      </Link>
      <PageHeader
        title={process?.name ?? "Process unavailable"}
        subtitle={`PID ${pid} · Started ${Number.isSafeInteger(start) ? new Date(start / 1000).toLocaleString() : "—"}`}
      />
      <SectionCard
        accent="disk"
        title={
          selected
            ? `Disk I/O · ${formatClockSeconds(selected.fromMs)} to ${formatClockSeconds(selected.toMs)}`
            : "Disk I/O"
        }
      >
        {history.isError ? (
          <p className="text-[12px] text-muted-foreground">
            Couldn't load process history.
          </p>
        ) : !process ? (
          <p className="text-[12px] text-muted-foreground">
            {history.isPending
              ? "Loading process history…"
              : "This process identity has no retained measurements in this interval. A reused PID does not identify the same process."}
          </p>
        ) : (
          <>
            <dl className="grid grid-cols-2 gap-x-6 gap-y-3 text-[12px] sm:grid-cols-3">
              {(!selected && live
                ? [
                    ["Read/s now", formatRate(live.disk_read_bps)],
                    ["Write/s now", formatRate(live.disk_write_bps)],
                    ["Total/s now", formatRate(diskTotal(live))],
                  ]
                : []
              ).map(([label, value]) => (
                <div key={label}>
                  <dt className="text-muted-foreground">{label}</dt>
                  <dd className="figures mt-1">{value}</dd>
                </div>
              ))}
              {retained &&
                [
                  ["Read", formatBytes(retained.read_bytes)],
                  ["Written", formatBytes(retained.write_bytes)],
                  ["Total", formatBytes(requestedBytes(retained))],
                  ["Average read/s", formatRate(retained.read_avg_bps)],
                  ["Average write/s", formatRate(retained.write_avg_bps)],
                  ["Average total/s", formatRate(retained.total_avg_bps)],
                ].map(([label, value]) => (
                  <div key={label}>
                    <dt className="text-muted-foreground">{label}</dt>
                    <dd className="figures mt-1">{value}</dd>
                  </div>
                ))}
            </dl>
            <p className="text-[12px] text-muted-foreground">
              {retained && !retained.running ? "Exited · " : ""}Requested bytes
              across all devices on this Mac.{" "}
              {history.data &&
                `Averages cover ${formatSpan(history.data.covered_ms)} of process sampling in the selected interval.`}
            </p>
            <p className="text-[12px] text-muted-foreground">
              {selected
                ? `Average CPU ${formatPercent(retained?.cpu_avg_pct ?? null)} · Peak memory ${formatBytes(retained?.mem_peak_bytes ?? null)}`
                : `Current CPU ${formatPercent(live?.cpu_pct ?? null)} · Memory ${formatBytes(live?.mem_bytes ?? null)}`}
            </p>
          </>
        )}
      </SectionCard>
    </div>
  );
}
