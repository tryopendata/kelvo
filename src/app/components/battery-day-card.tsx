import { formatHoursMinutes, formatPercent, MISSING } from "@core/format";
import type { ReactNode } from "react";
import { useBatterySummary } from "~/hooks/use-battery";
import { useHistoryHealth } from "~/hooks/use-history-health";
import { ChartRangeNotice, ChartRangeProvider } from "~/stores/chart-range";
import type { Corner } from "~/widgets/lib/accent";
import { StatStrip } from "~/widgets/stat-strip";
import { BatteryRangeHistory } from "./battery-range-history";
import { HistoryNotices } from "./history-notices";
import { SectionCard } from "./section-card";
import { WindowControl } from "./window-control";

const wh = (v: number | null) => (v === null ? MISSING : v.toFixed(1));

/**
 * The "Battery, last 24 hours" section, shared by Power & Sensors and
 * the Battery page (plan 4.10, 4.13). The header carries a strip
 * of five figures unless `aside` replaces it (the Battery page shows most of
 * them in its ring cards already).
 */
export function BatteryDayCard({
  origin = "tl",
  aside,
}: {
  origin?: Corner;
  aside?: ReactNode;
}) {
  const b = useBatterySummary();

  // With the store unavailable the bars still come back, from the engine's
  // last hour (D-092): the banner is the store's state, not the read's.
  const history = useHistoryHealth();
  const failure = history.error;
  return (
    <ChartRangeProvider>
      <SectionCard
        accent="battery"
        origin={origin}
        title="Battery charge"
        aside={
          aside ?? (
            <StatStrip
              items={[
                { label: "Charge", value: formatPercent(b.charge) },
                { label: "Health", value: formatPercent(b.health) },
                {
                  label: "Cycles",
                  value: b.cycles === null ? MISSING : String(b.cycles),
                },
                {
                  label: "Remaining",
                  value:
                    b.timeRemainingMin === null
                      ? MISSING
                      : formatHoursMinutes(b.timeRemainingMin * 60_000),
                },
                {
                  label: "Full charge",
                  value: wh(b.capacityWh),
                  unit: b.designWh === null ? "Wh" : `of ${wh(b.designWh)} Wh`,
                },
              ]}
            />
          )
        }
      >
        {failure && (
          // Minute history keeps going through a low-disk pause, and a trim
          // never reaches into the last 24 hours, so only the banner applies.
          <HistoryNotices health={undefined} error={failure} />
        )}
        <div className="flex justify-end">
          <WindowControl />
        </div>
        <ChartRangeNotice />
        <BatteryRangeHistory />
        <p className="text-[11px] text-muted-foreground">
          Charge, health and battery details above are current.
        </p>
      </SectionCard>
    </ChartRangeProvider>
  );
}
