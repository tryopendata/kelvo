import type { ByteUnits } from "@core/format";
import type {
  HistoricalUsageFigures,
  HistoricalUsageMetric,
} from "@core/generated/bindings";
import {
  historicalCoverage,
  historicalDelta,
  historicalFigure,
  historicalValue,
} from "@core/historical-usage";
import { APP_CELL } from "~/components/app-table";
import { TableCell } from "~/components/ui/table";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "~/components/ui/tooltip";

export function HistoricalUsageCells({
  figures,
  columns,
  units,
  spanMs,
  previous,
  compare = false,
}: {
  figures: HistoricalUsageFigures;
  columns: readonly HistoricalUsageMetric[];
  units: ByteUnits;
  spanMs: number;
  previous?: HistoricalUsageFigures | undefined;
  compare?: boolean;
}) {
  return columns.map((metric) => {
    const coverage = figures.coverage.find(
      (c) => c.metric === metric
    )?.coverage;
    const value = historicalFigure(
      historicalValue(figures, metric),
      metric,
      units
    );
    const priorCoverage = previous?.coverage.find(
      (resource) => resource.metric === metric
    )?.coverage;
    const priorValue = previous ? historicalValue(previous, metric) : null;
    const prior =
      priorValue === null
        ? "unavailable"
        : historicalFigure(priorValue, metric, units);
    const detail =
      metric === "network"
        ? `Received ${historicalFigure(figures.network_rx_bytes, "network", units)}; sent ${historicalFigure(figures.network_tx_bytes, "network", units)}`
        : historicalCoverage(coverage, spanMs);
    return (
      <TableCell key={metric} className={`${APP_CELL} figures text-right`}>
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              className="rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
              aria-label={`${metric}: ${value}; ${historicalCoverage(coverage, spanMs)}`}
            >
              {value}
              {coverage?.supported && !coverage.complete && (
                <span className="ml-1 text-[10px] text-muted-foreground">
                  partial
                </span>
              )}
            </button>
          </TooltipTrigger>
          <TooltipContent>
            {detail}. {historicalCoverage(coverage, spanMs)}
            {compare &&
              ` Previous: ${historicalCoverage(priorCoverage, spanMs)}. Differences compare observed values; coverage may differ.`}
            {metric === "cpu" && ". Percent of one CPU core."}
            {metric === "energy" &&
              ". Estimated CPU energy, not whole-device consumption."}
          </TooltipContent>
        </Tooltip>
        {compare && (
          <span className="block font-normal text-[10px] text-muted-foreground">
            Previous {prior}
            {priorCoverage?.supported && !priorCoverage.complete
              ? " (partial)"
              : ""}{" "}
            · {historicalDelta(figures, previous, metric, units)}
          </span>
        )}
      </TableCell>
    );
  });
}
