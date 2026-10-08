import { formatPercent, formatTemperature, formatWatts } from "@core/format";
import { Fragment } from "react";
import { cn } from "~/lib/utils";

/** Menu bar styles: "Combined", "Values" and "Graphs". */
export type TrayStyle = "combined" | "values" | "graphs";

/** Current values the tray shows. `null` is a gap, drawn as an empty track. */
export interface TrayValues {
  /** `cpu.total`, percent. */
  cpu: number | null;
  /** `gpu.util`, percent. */
  gpu: number | null;
  /** `mem.pressure`, percent. */
  mem: number | null;
  /** `thermal.hottest`, °C. */
  temp: number | null;
  /** `power.system`, watts. */
  power?: number | null;
  /** Recent `cpu.total` samples, oldest first, for the graphs style's sparkline. */
  cpuHistory?: readonly (number | null)[];
}

export interface TrayPreviewProps {
  style: TrayStyle;
  values: TrayValues;
  /** Clock text after the items, as the real menu bar shows it. */
  clock?: string;
  className?: string;
}

const BAR_H = 14;

function barHeight(pct: number | null): number {
  if (pct == null || !Number.isFinite(pct)) return 0;
  // Quantized to the pixel like the Rust renderer; any load shows a sliver.
  const h = Math.round((Math.min(100, Math.max(0, pct)) / 100) * BAR_H * 2) / 2;
  return pct > 0 ? Math.max(1, h) : 0;
}

/** The 17 x 14 pt combined icon: three 3 pt bars with 4 pt gaps, 30% track. */
function CombinedIcon({ values }: { values: TrayValues }) {
  const bars = [values.cpu, values.gpu, values.mem];
  return (
    <svg
      width="17"
      height="14"
      viewBox="0 0 17 14"
      aria-hidden
      className="shrink-0"
    >
      <g fill="currentColor">
        {bars.map((v, i) => {
          const h = barHeight(v);
          const x = i * 7;
          return (
            // biome-ignore lint/suspicious/noArrayIndexKey: fixed three-bar layout
            <Fragment key={i}>
              <rect x={x} y={0} width={3} height={BAR_H} rx={1} opacity={0.3} />
              {h > 0 && (
                <rect x={x} y={BAR_H - h} width={3} height={h} rx={1} />
              )}
            </Fragment>
          );
        })}
      </g>
    </svg>
  );
}

/** Samples across the sparkline, as the Rust renderer draws it. */
export const SPARK_SAMPLES = 20;

/**
 * Sparkline path in a 32 x 16 box: sample i at x 2 + 1.47 i, 0 to
 * 100% over y 14 to 2, newest at the right. A gap starts a new segment.
 */
export function sparkPath(samples: readonly (number | null)[]): string {
  const recent = samples.slice(-SPARK_SAMPLES);
  const offset = SPARK_SAMPLES - recent.length;
  let d = "";
  let open = false;
  recent.forEach((v, i) => {
    if (v == null || !Number.isFinite(v)) {
      open = false;
      return;
    }
    const x = (2 + (offset + i) * 1.47).toFixed(1);
    const y = (14 - (Math.min(100, Math.max(0, v)) / 100) * 12).toFixed(1);
    d += `${open ? "L" : "M"}${x} ${y} `;
    open = true;
  });
  return d.trim();
}

/** CPU line in its box ("Graphs"). */
function SparkIcon({ samples }: { samples: readonly (number | null)[] }) {
  return (
    <svg
      width="32"
      height="16"
      viewBox="0 0 32 16"
      aria-hidden
      className="shrink-0"
    >
      <rect
        x={0.5}
        y={0.5}
        width={31}
        height={15}
        rx={2}
        fill="none"
        stroke="currentColor"
        opacity={0.35}
      />
      <path
        d={sparkPath(samples)}
        fill="none"
        stroke="currentColor"
        strokeWidth={1.2}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
    </svg>
  );
}

/** Memory fill gauge, 6 x 16 ("Graphs"). */
function GaugeIcon({ pct }: { pct: number | null }) {
  const h =
    pct == null || !Number.isFinite(pct)
      ? 0
      : (Math.min(100, Math.max(0, pct)) / 100) * 13;
  return (
    <svg
      width="6"
      height="16"
      viewBox="0 0 6 16"
      aria-hidden
      className="shrink-0"
    >
      <rect
        x={0.5}
        y={0.5}
        width={5}
        height={15}
        rx={1.5}
        fill="none"
        stroke="currentColor"
        opacity={0.45}
      />
      {h > 0 && (
        <rect
          x={1.5}
          y={14.5 - h}
          width={3}
          height={h}
          rx={0.5}
          fill="currentColor"
        />
      )}
    </svg>
  );
}

/** Three letters stacked one per line, 6 px mono ("Values"). */
function Stacked({ text }: { text: string }) {
  return (
    <span
      aria-hidden
      className="inline-flex w-1.5 flex-col text-center font-tray text-[6px] leading-[6px]"
    >
      {text.split("").map((c, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: characters of a fixed label
        <span key={i}>{c}</span>
      ))}
    </span>
  );
}

function describe(values: TrayValues): string {
  const parts = [
    `CPU ${formatPercent(values.cpu)}`,
    `GPU ${formatPercent(values.gpu)}`,
    `memory ${formatPercent(values.mem)}`,
    formatTemperature(values.temp),
  ];
  return `Menu bar preview: ${parts.join(", ")}`;
}

/**
 * Simulated menu bar strip showing a tray style with live values. The
 * real item is drawn in Rust as a template image; this mirrors its
 * geometry so the onboarding and Settings choices preview truthfully.
 */
export function TrayPreview({
  style,
  values,
  clock = "10:40",
  className,
}: TrayPreviewProps) {
  return (
    <div
      role="img"
      aria-label={describe(values)}
      className={cn(
        "flex h-[22px] items-center justify-end gap-2.5 rounded-control border border-border-subtle bg-deep px-2.5 text-foreground",
        className
      )}
    >
      {style === "combined" ? (
        <>
          <CombinedIcon values={values} />
          <span className="font-tray text-[11px]">
            {formatTemperature(values.temp, { compact: true })}
          </span>
        </>
      ) : style === "graphs" ? (
        // The onboarding card shows CPU and memory; network's rates do not fit beside them.
        <>
          <span className="inline-flex items-center gap-1">
            <Stacked text="CPU" />
            <SparkIcon samples={values.cpuHistory ?? [values.cpu]} />
          </span>
          <span className="inline-flex items-center gap-1">
            <Stacked text="MEM" />
            <GaugeIcon pct={values.mem} />
            <span className="font-tray text-[11px]">
              {formatPercent(values.mem)}
            </span>
          </span>
        </>
      ) : (
        <>
          <Stacked text="CPU" />
          <span className="font-tray text-[11px]">
            {formatPercent(values.cpu)}
          </span>
          <Stacked text="GPU" />
          <span className="font-tray text-[11px]">
            {formatPercent(values.gpu)}
          </span>
          <Stacked text="MEM" />
          <span className="font-tray text-[11px]">
            {formatPercent(values.mem)}
          </span>
          <Stacked text="SOC" />
          <span className="font-tray text-[11px]">
            {formatTemperature(values.temp, { compact: true })}
          </span>
          {values.power !== undefined && (
            <>
              <Stacked text="PWR" />
              <span className="font-tray text-[11px]">
                {formatWatts(values.power).replace(" ", "")}
              </span>
            </>
          )}
        </>
      )}
      <span className="text-[11px] opacity-60">{clock}</span>
    </div>
  );
}
