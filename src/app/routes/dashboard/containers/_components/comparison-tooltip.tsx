import {
  type ComparisonMetric,
  pointCovered,
  seriesColor,
} from "@core/chart-math/series-lines";
import type {
  DockerContainer,
  DockerHistoryPoint,
} from "@core/generated/bindings";
import { Link, useLocation, useNavigate } from "react-router";
import {
  ChartTooltipShell,
  SeriesSwatch,
} from "~/components/charts/chart-tooltip";
import type { useBrushRange } from "~/stores/brush-store";
import { useSettingsStore } from "~/stores/settings-store";
import {
  containerReturnState,
  isContainerDetailClick,
} from "../_lib/navigation";
import type { ComparisonSeries } from "./comparison-chart";
export function ComparisonTooltip({
  rows,
  pinned,
  selection,
  cursor,
  from,
  to,
  duration,
  clock,
  link,
  displayName,
  statistics,
  format,
  metric,
}: {
  rows: readonly (ComparisonSeries & {
    point: DockerHistoryPoint | undefined;
    value: number | null;
  })[];
  pinned: boolean;
  selection: ReturnType<typeof useBrushRange>;
  cursor: number;
  from: number;
  to: number;
  duration: number;
  clock: (time: number) => string;
  link: (container: DockerContainer, point?: DockerHistoryPoint) => string;
  displayName: (container: DockerContainer) => string;
  statistics: (point: DockerHistoryPoint) => string;
  format: (value: number | null) => string;
  metric: ComparisonMetric;
}) {
  const navigate = useNavigate();
  const location = useLocation();
  const settings = useSettingsStore();
  return (
    <ChartTooltipShell
      className={`top-2 ${pinned ? "pointer-events-auto" : ""} ${(selection?.fromMs ?? cursor) > (from + to) / 2 ? "left-2" : "right-2"} max-w-full text-[11px]`}
    >
      <div>
        {clock(cursor)}
        {`–${clock(cursor + (rows.find((r) => r.point)?.point?.duration_ms ?? duration))}`}
      </div>
      {rows.map((row) => (
        <div key={row.container.id} className="flex items-center gap-2">
          <SeriesSwatch color={seriesColor(row.slot)} />
          {pinned ? (
            <Link
              className="flex-1 underline"
              to={link(row.container, row.point)}
              onClick={(event) => {
                if (!isContainerDetailClick(event)) return;
                event.preventDefault();
                void navigate(link(row.container, row.point), {
                  state: {
                    ...containerReturnState(location),
                    chartWindow:
                      settings.getState().snapshot?.settings.general
                        .chart_window,
                  },
                });
              }}
            >
              {displayName(row.container)}
            </Link>
          ) : (
            <span className="flex-1">{displayName(row.container)}</span>
          )}
          <span className="figures">
            {row.value === null
              ? "no data"
              : row.point
                ? statistics(row.point)
                : format(row.value)}
            {row.point && row.value !== null && !pointCovered(row.point, metric)
              ? " partial"
              : ""}
          </span>
        </div>
      ))}
      <span className="text-muted-foreground">min / avg / max</span>
    </ChartTooltipShell>
  );
}
