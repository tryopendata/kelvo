import {
  type ComparisonMetric,
  pointCovered,
  pointValue,
} from "@core/chart-math/series-lines";
import type {
  DockerContainer,
  DockerHistoryPoint,
} from "@core/generated/bindings";
import type { ComparisonSeries } from "./comparison-chart";
export function ComparisonValues({
  series,
  times,
  metric,
  displayName,
  clock,
  statistics,
  format,
}: {
  series: readonly ComparisonSeries[];
  times: readonly number[];
  metric: ComparisonMetric;
  displayName: (container: DockerContainer) => string;
  clock: (time: number) => string;
  statistics: (point: DockerHistoryPoint) => string;
  format: (value: number | null) => string;
}) {
  return (
    <div className="h-60 overflow-auto">
      <table
        className="figures w-full text-[11px]"
        aria-label="Container history values"
      >
        <caption className="text-left text-muted-foreground">
          Per bucket: min / avg / max
        </caption>
        <thead className="sticky top-0 bg-card">
          <tr>
            <th className="p-2 text-left">Time</th>
            {series.map((s) => (
              <th key={s.container.id} className="p-2 text-right">
                {displayName(s.container)}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {[...times].reverse().map((time) => (
            <tr key={time}>
              <td className="whitespace-nowrap p-2">{clock(time)}</td>
              {series.map((s) => {
                const point = s.points.find((p) => p.time_ms === time);
                const value = point ? pointValue(point, metric) : null;
                return (
                  <td
                    key={s.container.id}
                    className="whitespace-nowrap p-2 text-right"
                  >
                    {point && value !== null ? statistics(point) : format(null)}
                    {value !== null && point && !pointCovered(point, metric)
                      ? " partial"
                      : ""}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
