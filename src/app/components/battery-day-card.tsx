import { formatHoursMinutes, formatPercent, MISSING } from "@core/format";
import { CommandFailure } from "@core/transport";
import type { ReactNode } from "react";
import { useBatteryDetail, useBatteryHours } from "~/hooks/use-battery";
import { useHistoryHealth } from "~/hooks/use-history-health";
import { BatteryHistoryBars } from "~/widgets/battery-history-bars";
import type { Corner } from "~/widgets/lib/accent";
import { StatStrip } from "~/widgets/stat-strip";
import { HistoryNotices } from "./history-notices";
import { SectionCard } from "./section-card";

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
  const b = useBatteryDetail();
  const { hours, error } = useBatteryHours();
  // With the store unavailable the bars still come back, from the engine's
  // last hour (D-092): the banner is the store's state, not the read's.
  const history = useHistoryHealth();
  const failure =
    (error instanceof CommandFailure ? error.error : null) ?? history.error;
  return (
    <SectionCard
      accent="battery"
      origin={origin}
      title="Battery, last 24 hours"
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
      {hours ? (
        <BatteryHistoryBars hours={hours} annotations={[]} />
      ) : (
        <p className="flex h-[180px] items-center justify-center font-normal text-[12px] text-muted-foreground">
          {error ? "Battery history is unavailable." : "Loading history…"}
        </p>
      )}
      <div className="flex items-center gap-[18px] font-normal text-[11px] text-fg-subtle">
        <span className="inline-flex items-center gap-1.5">
          <span aria-hidden className="h-1 w-3.5 rounded-full bg-battery" />
          Charging
        </span>
        <span className="inline-flex items-center gap-1.5">
          <span aria-hidden className="size-2.5 rounded-mark bg-battery/75" />
          Charge at end of hour
        </span>
      </div>
    </SectionCard>
  );
}
