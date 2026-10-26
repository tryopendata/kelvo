import type { TimeRange } from "@core/brush";
import { dateInput, relativeWindow } from "@core/chart-range";
import type { ModuleCap } from "@core/generated/bindings";
import { historyUnavailable } from "@core/history-state";
import {
  ArrowLeft,
  ChevronLeft,
  ChevronRight,
  DownloadIcon,
} from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { HistoryNotices } from "~/components/history-notices";
import { Button } from "~/components/ui/button";
import { WindowControl } from "~/components/window-control";
import { useEvents } from "~/hooks/use-events";
import { useHistoryHealth } from "~/hooks/use-history-health";
import { useRangeNavigation } from "~/hooks/use-range-navigation";
import { useSeriesStats } from "~/hooks/use-series-stats";
import { useHostClock } from "~/lib/host-clock";
import { DockerTimelineLane } from "~/routes/dashboard/containers/_components/docker-surfaces";
import { useHost } from "~/stores/host-store";
import { useSettings } from "~/stores/settings-store";
import { HeatmapCard } from "./_components/heatmap-card";
import { LanesCard } from "./_components/lanes-card";
import { useExportCsv } from "./_hooks/use-export-csv";
import { type TimelineView, useTimelineData } from "./_hooks/use-timeline-data";
import { laneUnits } from "./_lib/format";
import { LANES } from "./_lib/lanes";
import { SUMMARY_METRICS } from "./_lib/summaries";
import { presentationSpan, SPAN_MS } from "./_lib/time";

const available = (cap: ModuleCap | undefined) =>
  cap !== undefined && typeof cap === "object" && "available" in cap;

/**
 * Timeline (plan 4.6): six stacked lanes over the last hour, 24
 * hours, 7 days or 30 days, from history through now (D-092), with
 * gap bands, Sleep and Wake markers and a synced crosshair; Export CSV of
 * what is shown; and the 30-day heatmap, whose cells open their hour here.
 * A heatmap cell older than 7 days opens 6 hours, a span with no preset:
 * the range control shows none selected and Live returns to 24h.
 */
export default function TimelineRoute() {
  const clock = useHostClock();
  const navigation = useRangeNavigation();
  const view: TimelineView = navigation.range ?? {
    windowMs: 86_400_000,
    endMs: null,
  };
  const span = presentationSpan(view.windowMs);
  const live = view.endMs === null;
  const caps = useHost((s) => s.capabilities);
  const unitSettings = useSettings((s) => s.units);
  const units = useMemo(() => laneUnits(unitSettings), [unitSettings]);

  // Lanes for modules the host has; a module switched off keeps its lane
  // and shows its gap.
  const lanes = useMemo(
    () => LANES.filter((l) => !caps || available(caps.modules[l.module])),
    [caps]
  );
  const [plotPx, setPlotPx] = useState(0);
  const data = useTimelineData(view, lanes, plotPx);
  const summaries = useSeriesStats(
    SUMMARY_METRICS,
    navigation.range ? { fromMs: data.fromMs, toMs: data.toMs } : null
  );
  const events = useEvents(view.windowMs, view.endMs);
  const timezone = Intl.DateTimeFormat().resolvedOptions().timeZone;
  const history = useHistoryHealth();
  const queryError = data.error?.error ?? null;
  // An unavailable store shows the banner; any other read error says so.
  const otherError =
    data.error && !historyUnavailable(data.error.error) ? data.error : null;

  const { exportCsv, exporting } = useExportCsv();
  const topRef = useRef<HTMLDivElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  // The heatmap sits below the lanes: bring them into view, and move focus
  // up with them so the keyboard and a screen reader follow.
  const selectRange = (range: TimeRange) =>
    navigation.apply({
      absolute: { from_ms: range.fromMs, to_ms: range.toMs },
    });
  const openHour = (next: {
    span: keyof typeof SPAN_MS;
    endMs: number | null;
  }) => {
    if (next.endMs === null)
      void navigation.apply(relativeWindow(SPAN_MS[next.span]));
    else
      selectRange({
        fromMs: next.endMs - SPAN_MS[next.span],
        toMs: next.endMs,
      });
    topRef.current?.scrollIntoView({ block: "start" });
    headingRef.current?.focus({ preventScroll: true });
  };

  const step = (direction: -1 | 1) => {
    const toMs = data.toMs + direction * view.windowMs;
    if (direction === 1 && toMs >= clock())
      void navigation.apply(relativeWindow(view.windowMs));
    else selectRange({ fromMs: Math.max(0, toMs - view.windowMs), toMs });
  };

  return (
    <div ref={topRef} className="flex flex-col gap-4">
      <header className="flex flex-wrap items-center gap-4">
        <div className="flex min-w-[240px] flex-1 flex-col gap-0.5">
          <h1
            ref={headingRef}
            tabIndex={-1}
            className="font-[590] text-[22px] tracking-[-0.022em] outline-none"
          >
            Timeline
          </h1>
          <span className="font-normal text-[12px] text-muted-foreground">
            {live ? "Live · " : "Recorded · "}
            <span className="figures">
              {dateInput(data.fromMs)} to {dateInput(data.toMs)}
            </span>
            {" · "}
            {timezone}
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={!navigation.canBack || navigation.pending}
            onClick={navigation.back}
          >
            <ArrowLeft />
            Back
          </Button>
          <WindowControl onApply={(value) => void navigation.apply(value)} />
          <Button
            variant="outline"
            size="icon"
            aria-label="Previous range"
            disabled={navigation.pending}
            onClick={() => step(-1)}
          >
            <ChevronLeft />
          </Button>
          {!live && (
            <Button
              variant="outline"
              size="icon"
              aria-label="Next range"
              disabled={navigation.pending}
              onClick={() => step(1)}
            >
              <ChevronRight />
            </Button>
          )}
          <Button
            variant="outline"
            size="sm"
            aria-pressed={live}
            disabled={navigation.pending}
            onClick={navigation.live}
          >
            Live
          </Button>
        </div>
        <Button
          variant="outline"
          size="sm"
          disabled={exporting || lanes.length === 0}
          onClick={() =>
            void exportCsv({
              windowMs: view.windowMs,
              fromMs: data.fromMs,
              toMs: data.toMs,
              lanes,
            })
          }
        >
          <DownloadIcon />
          Export CSV
        </Button>
      </header>

      <HistoryNotices
        health={history.health}
        error={queryError ?? history.error}
        fromMs={data.fromMs}
      />
      {otherError && (
        <p
          role="alert"
          className="rounded-tile border border-border bg-card px-4 py-3 font-normal text-[13px] text-fg-subtle"
        >
          History could not be read ({otherError.message}). Live values still
          work.
        </p>
      )}

      <LanesCard
        lanes={data.lanes}
        gaps={data.gaps}
        events={events}
        fromMs={data.fromMs}
        toMs={data.toMs}
        bucketMs={data.bucketMs}
        resolution={data.resolution}
        span={span}
        live={live}
        units={units}
        summaries={summaries.data}
        summaryError={summaries.isError}
        onPlotWidth={setPlotPx}
        onSelectRange={selectRange}
        selectionNowMs={clock()}
        retainedFromMs={history.health?.trimmed_before_ms ?? 0}
        loading={data.loading || navigation.pending}
        failed={data.error !== null}
      />

      {summaries.data &&
        (summaries.data.from_ms !== data.fromMs ||
          summaries.data.to_ms !== data.toMs) && (
          <p className="text-[11px] text-muted-foreground">
            Summary bucket coverage: {dateInput(summaries.data.from_ms)} to{" "}
            {dateInput(summaries.data.to_ms)} · {timezone}.
          </p>
        )}
      <DockerTimelineLane from={data.fromMs} to={data.toMs} />
      <HeatmapCard units={units} onOpen={openHour} />
    </div>
  );
}
