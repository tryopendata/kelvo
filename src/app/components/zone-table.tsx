import {
  formatTemperature,
  type TemperatureUnits,
  temperatureParts,
} from "@core/format";
import { FIELD_LABEL } from "~/widgets/lib/classes";
import { MeterTrack } from "~/widgets/meter-track";

export interface ZoneRow {
  /** Sensor key, also the row key ("PMU tdie4"). */
  key: string;
  /** Display name ("Zone 01"). */
  name: string;
  /** °C; `null` once the reading is stale. */
  now: number | null;
  /** Minimum and maximum over the chart window, °C. */
  min: number | null;
  max: number | null;
}

export interface ZoneExtra {
  /** "Battery", "SSD (NAND)", "Wi‑Fi module". */
  label: string;
  value: number | null;
}

export interface ZoneTableProps {
  /** In display order. The route sorts hottest first, at most every 10 s. */
  rows: readonly ZoneRow[];
  /** Other sensors shown under the table. */
  extras: readonly ZoneExtra[];
  units: TemperatureUnits;
  /** Range column heading, the chart window's short form ("15m", "1h"). */
  rangeLabel: string;
}

/** The bar spans 20 to 110 °C (design-system.md "Y scale"). */
const LO = 20;
const HI = 110;

function barFraction(c: number | null): number | null {
  return c == null ? null : (c - LO) / (HI - LO);
}

function range(
  min: number | null,
  max: number | null,
  units: TemperatureUnits
) {
  if (min == null || max == null) return "—";
  const lo = temperatureParts(min, { units }).value;
  const hi = temperatureParts(max, { units }).value;
  return `${lo}–${hi}`;
}

/**
 * Body rows are 24 px and the header 22 px; the table scrolls past ten and a
 * half rows, so the cut row says there is more.
 */
const ROW_PX = 24;
const HEAD_PX = 22;
const MAX_ROWS = 10.5;

/**
 * SoC thermal zones: zone, sensor key, a bar from 20 to 110 °C,
 * the current value and the range over the chart window. Temperatures are never colored
 * by value; the hottest zone is the top row. Past about ten zones the table
 * scrolls inside the card under a pinned header.
 */
export function ZoneTable({ rows, extras, units, rangeLabel }: ZoneTableProps) {
  // A scrolling list takes focus, so the keyboard can scroll it; WebKit does
  // not make scroll regions focusable on its own.
  const scrolls = rows.length > MAX_ROWS;
  return (
    <div className="flex flex-col gap-2.5">
      <div
        data-testid="zone-scroll"
        className="overflow-y-auto rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
        style={{ maxHeight: HEAD_PX + ROW_PX * MAX_ROWS }}
        {...(scrolls && {
          tabIndex: 0,
          role: "region",
          "aria-label": "SoC thermal zones",
        })}
      >
        <table className="w-full border-collapse text-[12px]">
          <colgroup>
            <col className="w-16" />
            <col className="w-[84px]" />
            <col />
            <col className="w-[52px]" />
            <col className="w-[60px]" />
          </colgroup>
          <thead className="sticky top-0 z-10 bg-card">
            <tr style={{ height: HEAD_PX }}>
              <th className={`${FIELD_LABEL} pb-1.5 text-left`}>Zone</th>
              <th className={`${FIELD_LABEL} pb-1.5 pl-2.5 text-left`}>
                Sensor
              </th>
              <th className="pb-1.5">
                <span className="sr-only">Range bar</span>
              </th>
              <th className={`${FIELD_LABEL} pb-1.5 text-right`}>Now</th>
              <th className={`${FIELD_LABEL} pb-1.5 text-right`}>
                {rangeLabel}
              </th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => (
              <tr key={r.key} style={{ height: ROW_PX }}>
                <td className="py-[3px] font-normal text-fg-subtle">
                  {r.name}
                </td>
                <td className="data-mono py-[3px] pl-2.5 text-[11px] text-muted-foreground">
                  {r.key}
                </td>
                <td className="px-2.5 py-[3px]">
                  <MeterTrack
                    fraction={barFraction(r.now)}
                    fill="var(--color-temp)"
                    transition
                  />
                </td>
                <td className="data-mono py-[3px] text-right text-foreground">
                  {formatTemperature(r.now, { units })}
                </td>
                <td className="data-mono py-[3px] text-right text-[11px] text-muted-foreground">
                  {range(r.min, r.max, units)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {extras.length > 0 && (
        <div className="flex flex-wrap gap-5 border-border-subtle border-t pt-2.5">
          {extras.map((x) => (
            <span
              key={x.label}
              className="font-normal text-[12px] text-fg-subtle"
            >
              {x.label}{" "}
              <span className="data-mono text-foreground">
                {formatTemperature(x.value, { units })}
              </span>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}
