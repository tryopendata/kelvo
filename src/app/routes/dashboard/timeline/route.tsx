import { formatMarketingMemory } from "@core/format";
import type { ModuleCap } from "@core/generated/bindings";
import { historyUnavailable } from "@core/history-state";
import { ChevronLeftIcon, ChevronRightIcon, DownloadIcon } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { HistoryNotices } from "~/components/history-notices";
import { SegmentedControl } from "~/components/segmented-control";
import { Button } from "~/components/ui/button";
import { useEvents } from "~/hooks/use-events";
import { useHistoryHealth } from "~/hooks/use-history-health";
import { useHostRecord } from "~/hooks/use-host-record";
import { useHost } from "~/stores/host-store";
import { useSettings } from "~/stores/settings-store";
import { HeatmapCard } from "./_components/heatmap-card";
import { LanesCard } from "./_components/lanes-card";
import { useExportCsv } from "./_hooks/use-export-csv";
import { type TimelineView, useTimelineData } from "./_hooks/use-timeline-data";
import { laneUnits } from "./_lib/format";
import { LANES } from "./_lib/lanes";
import { liveView, rangeSubtitle, SPAN_MS, stepView } from "./_lib/time";

const SPANS = [
  { value: "1h", label: "1h" },
  { value: "24h", label: "24h" },
  { value: "7d", label: "7d" },
  { value: "30d", label: "30d" },
] as const;

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
  const [view, setView] = useState<TimelineView>({ span: "24h", endMs: null });
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
  const events = useEvents(SPAN_MS[view.span], view.endMs);
  const subtitle = rangeSubtitle(view.span, data.fromMs, data.toMs, live);
  const history = useHistoryHealth();
  const queryError = data.error?.error ?? null;
  // An unavailable store shows the banner; any other read error says so.
  const otherError =
    data.error && !historyUnavailable(data.error.error) ? data.error : null;

  const host = useHostRecord();
  const memTotal = host
    ? formatMarketingMemory(host.info.mem_total_bytes)
    : null;

  const { exportCsv, exporting } = useExportCsv();
  const topRef = useRef<HTMLDivElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  // The heatmap sits below the lanes: bring them into view, and move focus
  // up with them so the keyboard and a screen reader follow.
  const openHour = useCallback((next: TimelineView) => {
    setView(next);
    topRef.current?.scrollIntoView({ block: "start" });
    headingRef.current?.focus({ preventScroll: true });
  }, []);

  const step = (direction: -1 | 1) =>
    setView((v) =>
      stepView(v.span, v.endMs ?? data.toMs, direction, Date.now())
    );

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
            {subtitle.lead}
            <span className="figures">{subtitle.times}</span>
          </span>
        </div>
        <SegmentedControl
          options={SPANS}
          value={view.span}
          onChange={(span) => setView((v) => ({ ...v, span }))}
          size="md"
          ariaLabel="Range"
        />
        <div className="flex items-center gap-1">
          <Button
            variant="outline"
            size="icon"
            aria-label="Previous range"
            onClick={() => step(-1)}
          >
            <ChevronLeftIcon />
          </Button>
          {!live && (
            <Button
              variant="outline"
              size="icon"
              aria-label="Next range"
              onClick={() => step(1)}
            >
              <ChevronRightIcon />
            </Button>
          )}
        </div>
        <Button
          variant="outline"
          size="sm"
          aria-pressed={live}
          onClick={() => setView((v) => liveView(v.span))}
        >
          Live
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={exporting || lanes.length === 0}
          onClick={() =>
            void exportCsv({
              span: view.span,
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
        span={view.span}
        live={live}
        units={units}
        memTotal={memTotal}
        onPlotWidth={setPlotPx}
      />

      <HeatmapCard units={units} onOpen={openHour} />
    </div>
  );
}
