import {
  type buildSeriesRuns,
  seriesColor,
} from "@core/chart-math/series-lines";
import type { ComparisonSeries } from "./comparison-chart";
export function ComparisonLines({
  plotSeries,
  active,
  onFocus,
  plotWidth,
}: {
  plotSeries: readonly (ComparisonSeries & {
    geometry: ReturnType<typeof buildSeriesRuns>;
  })[];
  active: string | null;
  onFocus: (id: string | null) => void;
  plotWidth: number;
}) {
  return (
    <>
      {" "}
      {plotSeries.map((s) => (
        <g
          key={s.container.id}
          opacity={active && active !== s.container.id ? 0.3 : 1}
          onPointerEnter={() => onFocus(s.container.id)}
        >
          {/* Each band spans its own bucket; missing spans remain empty. */}
          <path
            d={s.geometry.bands
              .filter((band) => !band.partial)
              .map(
                (band) =>
                  `M${band.x},${band.maximumY + 1}h${band.width}V${band.minimumY + 1}h${-band.width}Z`
              )
              .join(" ")}
            fill={seriesColor(s.slot)}
            fillOpacity={0.2}
          />
          <path
            d={s.geometry.bands
              .filter((band) => band.partial)
              .map(
                (band) => `M${band.x},${band.maximumY + 1}V${band.minimumY + 1}`
              )
              .join(" ")}
            fill="none"
            stroke={seriesColor(s.slot)}
            strokeOpacity={0.2}
            vectorEffect="non-scaling-stroke"
          />
          {s.geometry.runs.map((run) => (
            <path
              key={run[0]?.time}
              d={run
                .map((p, index) => `${index ? "L" : "M"}${p.x},${p.y + 1}`)
                .join(" ")}
              fill="none"
              stroke={seriesColor(s.slot)}
              strokeWidth={active === s.container.id ? 2 : 1}
              strokeLinecap="round"
              strokeLinejoin="round"
              vectorEffect="non-scaling-stroke"
            />
          ))}
          {s.geometry.dots.map((dot) => (
            <ellipse
              key={dot.time}
              cx={dot.x}
              cy={dot.y + 1}
              rx={(3 * 1000) / plotWidth}
              ry={3}
              fill={seriesColor(s.slot)}
              vectorEffect="non-scaling-stroke"
            />
          ))}
        </g>
      ))}
    </>
  );
}
