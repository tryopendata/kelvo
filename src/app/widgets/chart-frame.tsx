import type { CSSProperties, ReactNode, Ref } from "react";
import { WindowTicks } from "./window-ticks";

/** Shared plot geometry for live area charts and retained comparisons. */
export const CHART_WIDTH = 1000;
export const DASHBOARD_CHART_HEIGHT = 220;
export const chartY = (value: number, ceiling: number, height: number) =>
  height - 1 - Math.max(0, Math.min(1, value / (ceiling || 1))) * (height - 2);

export interface ChartTick {
  value: number;
  label: string;
}

/** Owns axis typography, plot spacing and time labels, independently of series rendering. */
export function ChartFrame({
  height,
  ceiling,
  yTicks,
  xTicks,
  gutter = 32,
  plotRef,
  style,
  children,
}: {
  height: number;
  ceiling: number;
  yTicks?: readonly ChartTick[];
  xTicks?: readonly string[];
  gutter?: number;
  plotRef?: Ref<HTMLDivElement>;
  style?: CSSProperties;
  children: ReactNode;
}) {
  return (
    <div
      className="flex flex-col gap-1.5"
      style={{ ...style, paddingLeft: yTicks ? gutter : 0 }}
    >
      <div ref={plotRef} className="relative" style={{ height }}>
        {children}
        {yTicks?.map((tick) => (
          <span
            key={tick.value}
            aria-hidden
            className="figures absolute -translate-y-1/2 whitespace-nowrap text-[10px] text-fg-faint"
            style={{ left: -gutter, top: chartY(tick.value, ceiling, height) }}
          >
            {tick.label}
          </span>
        ))}
      </div>
      {xTicks && xTicks.length > 0 && <WindowTicks ticks={xTicks} />}
    </div>
  );
}

/** Horizontal rules use the same scale as marks; the zero baseline has its own token. */
export function ChartGrid({
  values,
  ceiling,
  height,
}: {
  values: readonly number[];
  ceiling: number;
  height: number;
}) {
  return (
    <>
      {values.length > 0 && (
        <path
          d={values
            .map(
              (value) =>
                `M0 ${chartY(value, ceiling, height).toFixed(1)}H${CHART_WIDTH}`
            )
            .join("")}
          stroke="var(--color-grid)"
          vectorEffect="non-scaling-stroke"
        />
      )}
      <path
        d={`M0 ${height - 0.5}H${CHART_WIDTH}`}
        stroke="var(--color-axis)"
        vectorEffect="non-scaling-stroke"
      />
    </>
  );
}
