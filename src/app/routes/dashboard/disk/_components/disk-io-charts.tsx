import { rangeSlots, type TimeRange } from "@core/brush";
import { formatHistoryTime, formatRate } from "@core/format";
import type { DiskIoPage, DiskIoPoint } from "@core/generated/bindings";
import { gapBands } from "@core/history-state";
import { useCallback, useLayoutEffect, useMemo, useRef } from "react";
import { BrushOverlay } from "~/components/brush-overlay";
import { ChartTooltipShell } from "~/components/charts/chart-tooltip";
import { PLOT_INSET } from "~/components/charts/layout";
import { SegmentedControl } from "~/components/segmented-control";
import { useChartBrush } from "~/hooks/use-chart-brush";
import { useNiceCeiling } from "~/hooks/use-nice-ceiling";
import { GapBands } from "~/widgets/gap-band";
import { FIELD_LABEL } from "~/widgets/lib/classes";
import { MirrorBars } from "~/widgets/mirror-bars";
import { WindowTicks } from "~/widgets/window-ticks";

export type IoSecondary = "time" | "ops";
const OPTIONS = [
  { value: "time", label: "Average request time" },
  { value: "ops", label: "Operations/s" },
] as const;
export const requestTime = (ms: number | null, ops: number | null) =>
  ms === null
    ? ops === 0
      ? "— · No completed operations"
      : "— · Unavailable"
    : milliseconds(ms);
const operations = (n: number) =>
  `${n.toLocaleString("en-US", { maximumFractionDigits: 1 })} ops/s`;
const milliseconds = (n: number) =>
  n > 0 && n < 0.01
    ? "<0.01 ms"
    : `${n.toLocaleString("en-US", { maximumFractionDigits: 2 })} ms`;

/** Two views of the same backend buckets, with a shared DOM crosshair. */
export function DiskIoCharts({
  page,
  range,
  secondary,
  onSecondary,
}: {
  page: DiskIoPage;
  range: TimeRange;
  secondary: IoSecondary;
  onSecondary: (v: IoSecondary) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const hovered = useRef<number | null>(null);
  const first = Math.floor(range.fromMs / page.bucket_ms) * page.bucket_ms;
  const count = Math.max(1, Math.ceil((range.toMs - first) / page.bucket_ms));
  const end = first + count * page.bucket_ms;
  const dates = range.toMs - range.fromMs >= 86_400_000;
  const points = useMemo(() => {
    const byTime = new Map(page.points.map((p) => [p.t, p]));
    return Array.from(
      { length: count },
      (_, i) => byTime.get(first + i * page.bucket_ms) ?? null
    );
  }, [page.points, page.bucket_ms, count, first]);
  const paint = useCallback(
    // biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Existing cohesive logic; split when substantially changed (architecture refactor section 10).
    (time: number | null) => {
      hovered.current = time;
      const root = host.current;
      if (!root) return;
      const i =
        time === null ? -1 : Math.floor((time - first) / page.bucket_ms);
      const point = points[i];
      const valid = i >= 0 && i < count;
      root
        .querySelectorAll<HTMLElement>("[data-io-crosshair]")
        .forEach((el) => {
          el.hidden = !valid;
          el.style.left = `calc(${PLOT_INSET}px + (100% - ${PLOT_INSET}px) * ${(i + 0.5) / count})`;
        });
      const tip = root.querySelector<HTMLElement>('[role="tooltip"]');
      if (!tip) return;
      tip.hidden = !valid;
      const set = (field: string, value: string) => {
        const el = tip.querySelector(`[data-io-field="${field}"]`);
        if (el) el.textContent = value;
      };
      set(
        "time",
        valid
          ? `${formatHistoryTime(first + i * page.bucket_ms, dates)}–${formatHistoryTime(first + (i + 1) * page.bucket_ms, dates)}`
          : ""
      );
      set("read", formatRate(point?.read_bps ?? null));
      set("write", formatRate(point?.write_bps ?? null));
      set(
        "readOps",
        point?.read_ops_per_sec == null
          ? "—"
          : operations(point.read_ops_per_sec)
      );
      set(
        "writeOps",
        point?.write_ops_per_sec == null
          ? "—"
          : operations(point.write_ops_per_sec)
      );
      set(
        "readTime",
        requestTime(
          point?.read_request_ms ?? null,
          point?.read_ops_per_sec ?? null
        )
      );
      set(
        "writeTime",
        requestTime(
          point?.write_request_ms ?? null,
          point?.write_ops_per_sec ?? null
        )
      );
    },
    [count, first, page.bucket_ms, points, dates]
  );
  useLayoutEffect(() => paint(hovered.current), [paint]);
  const gap = useMemo(
    () => gapBands(page.gaps, first, range.toMs, { module: "disk" }),
    [page.gaps, first, range.toMs]
  );
  const last = points.at(-1);
  return (
    <div ref={host} className="relative flex flex-col gap-3">
      <IoPlot
        label="Disk I/O"
        points={points}
        up="read_bps"
        down="write_bps"
        page={page}
        first={first}
        count={count}
        format={formatRate}
        minimum={1e6}
        gaps={gap}
        onHover={paint}
      />
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <SegmentedControl
          options={OPTIONS}
          value={secondary}
          onChange={onSecondary}
          ariaLabel="Storage response metric"
        />
        <p
          className="figures text-[11px] text-muted-foreground"
          data-testid="disk-response-current"
        >
          {secondary === "time"
            ? `Read ${requestTime(last?.read_request_ms ?? null, last?.read_ops_per_sec ?? null)} · Write ${requestTime(last?.write_request_ms ?? null, last?.write_ops_per_sec ?? null)}`
            : `Read ${last?.read_ops_per_sec == null ? "—" : operations(last.read_ops_per_sec)} · Write ${last?.write_ops_per_sec == null ? "—" : operations(last.write_ops_per_sec)}`}
        </p>
      </div>
      <IoPlot
        label={secondary === "time" ? "Average request time" : "Operations/s"}
        points={points}
        up={secondary === "time" ? "read_request_ms" : "read_ops_per_sec"}
        down={secondary === "time" ? "write_request_ms" : "write_ops_per_sec"}
        page={page}
        first={first}
        count={count}
        format={secondary === "time" ? milliseconds : operations}
        minimum={secondary === "time" ? 1 : 100}
        gaps={gap}
        onHover={paint}
      />
      <WindowTicks
        ticks={[first, first + (end - first) / 2, end].map((t) =>
          formatHistoryTime(t, dates)
        )}
        style={{ paddingLeft: PLOT_INSET }}
      />
      <p className="font-normal text-[11px] text-muted-foreground">
        Completed driver requests, not application response time or queueing
        delay. Chart ceilings adapt to the data, not drive capacity.
      </p>
      <ChartTooltipShell hidden className="top-0 right-0 w-64 max-w-full">
        <span data-io-field="time" className="figures text-[12px]" />
        <div className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-1 text-[12px]">
          <span>Read</span>
          <span data-io-field="read" className="figures" />
          <span>Write</span>
          <span data-io-field="write" className="figures" />
          <span>Read operations</span>
          <span data-io-field="readOps" className="figures" />
          <span>Write operations</span>
          <span data-io-field="writeOps" className="figures" />
          <span>Read request</span>
          <span data-io-field="readTime" className="figures" />
          <span>Write request</span>
          <span data-io-field="writeTime" className="figures" />
        </div>
      </ChartTooltipShell>
    </div>
  );
}

function IoPlot({
  label,
  points,
  up,
  down,
  page,
  first,
  count,
  format,
  minimum,
  gaps,
  onHover,
}: {
  label: string;
  points: (DiskIoPoint | null)[];
  up: keyof DiskIoPoint;
  down: keyof DiskIoPoint;
  page: DiskIoPage;
  first: number;
  count: number;
  format: (n: number) => string;
  minimum: number;
  gaps: ReturnType<typeof gapBands>;
  onHover: (t: number | null) => void;
}) {
  const { selected } = useChartBrush(true);
  const upper = points.map((p) => p?.[up] ?? null);
  const lower = points.map((p) => p?.[down] ?? null);
  const span = count * page.bucket_ms;
  const end = first + span;
  const upMax = useNiceCeiling(upper, end, minimum, span);
  const downMax = useNiceCeiling(lower, end, minimum, span);
  const highlight = selected
    ? rangeSlots(selected, first, page.bucket_ms, count)
    : null;
  return (
    <div className="relative" style={{ paddingLeft: PLOT_INSET }}>
      <div
        aria-hidden
        className="absolute inset-y-0 left-0 flex w-22 flex-col justify-between"
      >
        <span className={FIELD_LABEL}>
          ↑ Read<span className="block">{format(upMax)}</span>
        </span>
        <span className={FIELD_LABEL}>
          <span className="block">{format(downMax)}</span>↓ Write
        </span>
      </div>
      <MirrorBars
        up={upper}
        down={lower}
        intervalMs={page.bucket_ms}
        tEndMs={end}
        accent="disk"
        ariaLabel={`${label}: read above, write below. Drag or use arrow keys and Enter to select a range.`}
        upMax={upMax}
        downMax={downMax}
        upHeight={48}
        downHeight={48}
        highlight={highlight}
      />
      <BrushOverlay
        firstMs={first}
        spanMs={span}
        height={97}
        onHoverTime={onHover}
      />
      <div
        className="pointer-events-none absolute inset-y-0 right-0"
        style={{ left: PLOT_INSET }}
      >
        <GapBands gaps={gaps} rangeFromMs={first} rangeToMs={end} />
      </div>
      <div
        data-io-crosshair
        hidden
        aria-hidden
        className="pointer-events-none absolute inset-y-0 w-px bg-fg-subtle"
      />
    </div>
  );
}
